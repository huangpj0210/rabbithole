//! SSH Tunnel Gateway:任意 OpenSSH 客户端 `ssh -R` 直接注册远程转发。
//!
//! - 认证:用户名 = 客户端 ID;密码 = 客户端密钥(常量时间比较);
//!   或 authorized_keys 公钥(用户名仍须为已启用客户端)。
//! - `ssh -R [addr:]port:host:port user@gw` → 服务端绑定监听;用户连接到来时
//!   经 SSH `forwarded-tcpip` 通道送回 ssh 客户端,由其对 host:port 发起真实连接。
//! - 端口 0 → 服务端自动分配;SSH 连接断开即释放该连接注册的全部监听。

use crate::state::AppState;
use russh::server::Handle as SshHandle;
use russh::server::{Auth, Handler, Session};
use russh::ChannelMsg;
use ssh_key::{LineEnding, PrivateKey, PublicKey};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

pub struct SshGateway {
    state: Arc<AppState>,
    host_key_dir: PathBuf,
}

impl SshGateway {
    pub fn new(state: Arc<AppState>, host_key_dir: PathBuf) -> Self {
        Self {
            state,
            host_key_dir,
        }
    }

    pub async fn serve(self) -> anyhow::Result<()> {
        let cfg = self.state.cfg.ssh_gateway.clone();
        let authorized_keys = load_authorized_keys(cfg.authorized_keys_file.as_ref());
        let host_key = load_or_gen_host_key(&self.host_key_dir)?;
        let scfg = Arc::new(russh::server::Config {
            auth_rejection_time: std::time::Duration::from_secs(1),
            keys: vec![host_key],
            ..Default::default()
        });

        let bind_ip = if cfg.bind_addr.is_empty() {
            "0.0.0.0".to_string()
        } else {
            cfg.bind_addr.clone()
        };
        let addr = format!("{bind_ip}:{}", cfg.bind_port);
        let l = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!(%addr, "ssh gateway up");
        loop {
            let (sock, _peer) = match l.accept().await {
                Ok(v) => v,
                Err(_) => continue,
            };
            let state = self.state.clone();
            let keys = authorized_keys.clone();
            let cfg2 = scfg.clone();
            tokio::spawn(async move {
                let forwards: ForwardMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
                let session = SshSession {
                    state,
                    client_id: String::new(),
                    authorized_keys: keys,
                    forwards: forwards.clone(),
                };
                // 会话结束(连接断开)即释放该连接注册的全部监听
                let running = match russh::server::run_stream(cfg2, sock, session).await {
                    Ok(r) => r,
                    Err(_) => return,
                };
                let _ = running.await;
                for (_, t) in forwards.lock().await.drain() {
                    t.cancel();
                }
                tracing::info!("ssh gateway session closed");
            });
        }
    }
}

fn load_authorized_keys(path: Option<&PathBuf>) -> Arc<Vec<PublicKey>> {
    let Some(p) = path else {
        return Arc::new(Vec::new());
    };
    let mut out = Vec::new();
    if let Ok(text) = std::fs::read_to_string(p) {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            match PublicKey::from_openssh(line) {
                Ok(k) => out.push(k),
                Err(e) => tracing::warn!(%e, line, "ssh gateway: bad authorized_keys line"),
            }
        }
    } else {
        tracing::warn!(path = %p.display(), "ssh gateway: authorized_keys unreadable");
    }
    Arc::new(out)
}

fn load_or_gen_host_key(dir: &PathBuf) -> anyhow::Result<PrivateKey> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("ssh_host_ed25519");
    if let Ok(text) = std::fs::read_to_string(&path) {
        return PrivateKey::from_openssh(&text)
            .map_err(|e| anyhow::anyhow!("ssh host key parse: {e}"));
    }
    let mut rng = rand_010::rng();
    let key = PrivateKey::random(&mut rng, ssh_key::Algorithm::Ed25519)
        .map_err(|e| anyhow::anyhow!("ssh host key gen: {e}"))?;
    let pem = key.to_openssh(LineEnding::LF)?;
    std::fs::write(&path, pem.as_bytes())?;
    tracing::info!(path = %path.display(), "ssh gateway: host key generated");
    Ok(key)
}

type ForwardMap = Arc<tokio::sync::Mutex<HashMap<u16, CancellationToken>>>;

struct SshSession {
    state: Arc<AppState>,
    client_id: String,
    authorized_keys: Arc<Vec<PublicKey>>,
    /// 本连接注册的监听:port → 取消器(断开级联释放)。
    forwards: ForwardMap,
}

impl Handler for SshSession {
    type Error = anyhow::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        let ok = match self.state.db.get_client(user).await {
            Ok(Some(c)) => {
                c.enabled
                    && rh_transport::constant_time_eq(c.secret.as_bytes(), password.as_bytes())
            }
            _ => false,
        };
        if ok {
            self.client_id = user.to_string();
            tracing::info!(client = user, "ssh gateway auth ok (password)");
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        public_key: &PublicKey,
    ) -> Result<Auth, Self::Error> {
        // PublicKey 的派生相等含注释字段; authorized_keys 行注释与线上无关,只比密钥本体
        let listed = self
            .authorized_keys
            .iter()
            .any(|k| k.key_data() == public_key.key_data());
        let client_ok = match self.state.db.get_client(user).await {
            Ok(Some(c)) => c.enabled,
            _ => false,
        };
        if listed && client_ok {
            self.client_id = user.to_string();
            tracing::info!(client = user, "ssh gateway auth ok (publickey)");
            Ok(Auth::Accept)
        } else {
            Ok(Auth::reject())
        }
    }

    /// 接受会话通道并"空跑"命令:隧道场景仅作会话保持,不真正执行。
    async fn channel_open_session(
        &mut self,
        _channel: russh::Channel<russh::server::Msg>,
        reply: russh::server::ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: russh::ChannelId,
        _data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let _ = session.channel_success(channel);
        Ok(())
    }

    async fn shell_request(
        &mut self,
        channel: russh::ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let _ = session.channel_success(channel);
        Ok(())
    }

    async fn tcpip_forward(
        &mut self,
        address: &str,
        port: &mut u32,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        if self.client_id.is_empty() {
            return Ok(false);
        }
        let bind_ip: std::net::IpAddr = if address.is_empty() {
            "127.0.0.1".parse().unwrap()
        } else {
            address
                .parse()
                .unwrap_or_else(|_| "127.0.0.1".parse().unwrap())
        };
        let requested = *port as u16;
        let bind_port = if requested == 0 {
            match self.state.proxies.allocate_port("tcp", bind_ip).await {
                Some(p) => p,
                None => return Ok(false),
            }
        } else {
            requested
        };
        let l = match tokio::net::TcpListener::bind((bind_ip, bind_port)).await {
            Ok(l) => l,
            Err(_) => return Ok(false),
        };
        let allocated = l.local_addr()?.port();
        *port = allocated as u32;
        let cancel = CancellationToken::new();
        self.forwards.lock().await.insert(allocated, cancel.clone());
        let handle = session.handle();
        let st = self.state.clone();
        let client_id = self.client_id.clone();
        // 通道回程的 connected_address 必须与注册串一致(OpenSSH 按其匹配转发)
        let registered = if address.is_empty() {
            "localhost".to_string()
        } else {
            address.to_string()
        };
        tokio::spawn(forward_accept(
            st, handle, l, cancel, client_id, allocated, registered,
        ));
        tracing::info!(client = %self.client_id, port = allocated, "ssh gateway forward registered");
        Ok(true)
    }

    async fn cancel_tcpip_forward(
        &mut self,
        _address: &str,
        port: u32,
        _session: &mut Session,
    ) -> Result<bool, Self::Error> {
        if let Some(t) = self.forwards.lock().await.remove(&(port as u16)) {
            t.cancel();
        }
        Ok(true)
    }
}

/// 监听接受循环:每个用户连接开一条 forwarded-tcpip 通道回 ssh 客户端。
async fn forward_accept(
    state: Arc<AppState>,
    handle: SshHandle,
    l: tokio::net::TcpListener,
    cancel: CancellationToken,
    client_id: String,
    port: u16,
    registered_addr: String,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(client = %client_id, port, "ssh gateway forward released");
                return;
            }
            acc = l.accept() => {
                let Ok((sock, peer)) = acc else { return };
                let h = handle.clone();
                let st2 = state.clone();
                let reg = registered_addr.clone();
                tokio::spawn(async move {
                    st2.metrics.totals.lock().unwrap().conns_current += 1;
                    let ch = match h
                        .channel_open_forwarded_tcpip(
                            reg,
                            port as u32,
                            peer.ip().to_string(),
                            peer.port() as u32,
                        )
                        .await
                    {
                        Ok(c) => c,
                        Err(_) => {
                            st2.metrics.totals.lock().unwrap().conns_current -= 1;
                            return;
                        }
                    };
                    pump(sock, ch).await;
                    st2.metrics.totals.lock().unwrap().conns_current -= 1;
                });
            }
        }
    }
}

/// 用户 TCP ↔ SSH 通道双向搬运(含半关闭)。
async fn pump(sock: tokio::net::TcpStream, mut channel: russh::Channel<russh::server::Msg>) {
    let (mut sr, mut sw) = sock.into_split();
    let mut writer = channel.make_writer();
    let t1 = tokio::spawn(async move {
        let mut buf = vec![0u8; 32 * 1024];
        loop {
            match sr.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if writer.write_all(&buf[..n]).await.is_err() {
                        break;
                    }
                }
            }
        }
        let _ = writer.shutdown().await;
    });
    let mut pending = Vec::new();
    loop {
        let msg = channel.wait().await;
        match msg {
            Some(ChannelMsg::Data { data }) => pending.extend_from_slice(&data),
            Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
            _ => {}
        }
        while !pending.is_empty() {
            match sw.write_buf(&mut &pending[..]).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = pending.drain(..n);
                }
            }
        }
    }
    let _ = t1.await;
    let _ = channel.close().await;
}
