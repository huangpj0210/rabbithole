//! KCP 载体:UDP 之上的可靠有序字节流(会话),会话之上再走 yamux 多路复用。
//!
//! - 服务端:单 UDP 端口,kcp 头部的 conv 字段做会话分离;未知 conv 的
//!   合法 kcp 报文触发建会话并交给 accept 队列。
//! - 客户端:随机 conv,直接开始发送(首包即隐式握手)。
//! - 会话驱动:10ms tick 调用 kcp.update;yamux 层 keepalive 维持活跃,
//!   SESSION_IDLE_TIMEOUT 内无任何入站报文则回收。

use crate::mux::{wrap_stream_as_client, wrap_stream_as_server};
use crate::{
    other_err, ClientTransportConfig, Incoming, Multiplexer, Protocol, ServerTransportConfig,
};
use kcp::{Kcp, KCP_OVERHEAD};
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, Notify};

const TICK: Duration = Duration::from_millis(10);
const MTU: usize = 1200;
const SESSION_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
const MAX_WRITE_QUEUE: usize = 256;

fn now_ms() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

// ---------------- 会话 IO(用户侧字节流) ----------------

struct Guarded {
    read_buf: VecDeque<u8>,
    read_waker: Option<Waker>,
    write_queue: VecDeque<Vec<u8>>,
    write_waker: Option<Waker>,
    closed: bool,
}

impl Guarded {
    fn wake_read(&mut self) {
        if let Some(w) = self.read_waker.take() {
            w.wake();
        }
    }
    fn wake_write(&mut self) {
        if let Some(w) = self.write_waker.take() {
            w.wake();
        }
    }
}

/// 会话共享状态:notify 独立于锁,避免借用临时 guard。
struct Shared {
    guarded: Mutex<Guarded>,
    notify: Notify,
}

/// KCP 会话的字节流句柄。
pub struct KcpIo {
    shared: Arc<Shared>,
}

impl AsyncRead for KcpIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let mut s = self.shared.guarded.lock().unwrap();
        if !s.read_buf.is_empty() {
            let n = s.read_buf.len().min(buf.remaining());
            let chunk: Vec<u8> = s.read_buf.drain(..n).collect();
            buf.put_slice(&chunk);
            Poll::Ready(Ok(()))
        } else if s.closed {
            Poll::Ready(Ok(()))
        } else {
            s.read_waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl AsyncWrite for KcpIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let mut s = self.shared.guarded.lock().unwrap();
        if s.closed {
            return Poll::Ready(Err(other_err("kcp session closed")));
        }
        if s.write_queue.len() >= MAX_WRITE_QUEUE {
            s.write_waker = Some(cx.waker().clone());
            return Poll::Pending;
        }
        s.write_queue.push_back(buf.to_vec());
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        let mut s = self.shared.guarded.lock().unwrap();
        s.closed = true;
        s.wake_read();
        Poll::Ready(Ok(()))
    }
}

// ---------------- kcp 输出 → UDP ----------------

struct UdpOutput {
    sock: Arc<UdpSocket>,
    peer: SocketAddr,
    /// 客户端 socket 已 connect:必须用 try_send 而非 try_send_to(否则 EISCONN)。
    connected: bool,
}

impl Write for UdpOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let r = if self.connected {
            self.sock.try_send(buf)
        } else {
            self.sock.try_send_to(buf, self.peer)
        };
        match r {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(buf.len()),
            Err(e) => Err(e),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// ---------------- 会话任务 ----------------

enum SessionCmd {
    Packet(Vec<u8>),
}

async fn run_session<O: std::io::Write>(
    mut kcp: Kcp<O>,
    mut inbound: mpsc::Receiver<SessionCmd>,
    shared: Arc<Shared>,
) {
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_activity = Instant::now();

    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            cmd = inbound.recv() => {
                match cmd {
                    Some(SessionCmd::Packet(pkt)) => {
                        last_activity = Instant::now();
                        let _ = kcp.input(&pkt);
                    }
                    None => break,
                }
            }
            _ = shared.notify.notified() => {}
        }
        // 用户写队列 → kcp
        {
            let mut s = shared.guarded.lock().unwrap();
            while let Some(chunk) = s.write_queue.pop_front() {
                if kcp.send(&chunk).is_err() {
                    s.closed = true;
                    s.wake_read();
                    s.wake_write();
                    return;
                }
            }
            s.wake_write();
        }
        // 驱动 kcp(重传/探测/输出)
        if kcp.update(now_ms()).is_err() {
            break;
        }
        // kcp 接收 → 读缓冲
        while let Ok(size) = kcp.peeksize() {
            let mut buf = vec![0u8; size];
            match kcp.recv(&mut buf) {
                Ok(n) => {
                    buf.truncate(n);
                    let mut s = shared.guarded.lock().unwrap();
                    s.read_buf.extend(buf);
                    s.wake_read();
                }
                Err(_) => break,
            }
        }
        if last_activity.elapsed() > SESSION_IDLE_TIMEOUT {
            tracing::debug!("kcp session idle timeout");
            break;
        }
    }
    let mut s = shared.guarded.lock().unwrap();
    s.closed = true;
    s.wake_read();
    s.wake_write();
}

// ---------------- 服务端监听 ----------------

pub struct KcpListener {
    local: SocketAddr,
    accept_rx: mpsc::Receiver<Arc<Multiplexer>>,
}

impl KcpListener {
    pub fn local_addr(&self) -> SocketAddr {
        self.local
    }

    pub async fn accept(&mut self) -> std::io::Result<Incoming> {
        let mux = self
            .accept_rx
            .recv()
            .await
            .ok_or_else(|| other_err("kcp listener closed"))?;
        Ok(Incoming {
            mux,
            remote: self.local,
            protocol: Protocol::Kcp,
        })
    }
}

pub async fn listen(cfg: &ServerTransportConfig) -> std::io::Result<KcpListener> {
    let udp = Arc::new(UdpSocket::bind(cfg.bind_addr).await?);
    let local = udp.local_addr()?;
    let (accept_tx, accept_rx) = mpsc::channel::<Arc<Multiplexer>>(64);
    tokio::spawn(demux_loop(udp, accept_tx));
    Ok(KcpListener { local, accept_rx })
}

type SessionMap = Arc<Mutex<HashMap<u32, mpsc::Sender<SessionCmd>>>>;

async fn demux_loop(udp: Arc<UdpSocket>, accept_tx: mpsc::Sender<Arc<Multiplexer>>) {
    let sessions: SessionMap = Arc::new(Mutex::new(HashMap::new()));
    let mut buf = vec![0u8; 65536];
    loop {
        let (n, peer) = match udp.recv_from(&mut buf).await {
            Ok(x) => x,
            Err(e) => {
                tracing::warn!(error = %e, "kcp recv error");
                continue;
            }
        };
        if n < KCP_OVERHEAD {
            continue;
        }
        let pkt = &buf[..n];
        let conv = kcp::get_conv(pkt);
        // cmd 合法性:81(PUSH) 82(ACK) 83(WASK) 84(WINS)
        if !(81..=84).contains(&pkt[4]) {
            continue;
        }
        let route = {
            let mut g = sessions.lock().unwrap();
            match g.get(&conv) {
                Some(tx) if !tx.is_closed() => tx.clone(),
                _ => {
                    let (tx, rx) = mpsc::channel::<SessionCmd>(256);
                    g.insert(conv, tx.clone());
                    spawn_server_session(conv, rx, sessions.clone(), udp.clone(), peer, &accept_tx);
                    tx
                }
            }
        };
        let _ = route.try_send(SessionCmd::Packet(pkt.to_vec()));
    }
}

fn spawn_server_session(
    conv: u32,
    rx: mpsc::Receiver<SessionCmd>,
    sessions: SessionMap,
    udp: Arc<UdpSocket>,
    peer: SocketAddr,
    accept_tx: &mpsc::Sender<Arc<Multiplexer>>,
) {
    let (io, shared) = new_io();
    let output = UdpOutput {
        sock: udp,
        peer,
        connected: false,
    };
    let mut kcp = Kcp::new_stream(conv, output);
    kcp.set_mtu(MTU).ok();
    kcp.set_wndsize(256, 256);
    kcp.set_nodelay(true, 10, 2, true);
    kcp.set_maximum_resend_times(30);
    let cleanup_sessions = sessions.clone();
    tokio::spawn(async move {
        run_session(kcp, rx, shared).await;
        let mut g = cleanup_sessions.lock().unwrap();
        g.remove(&conv);
    });
    // yamux 服务端在会话字节流上收流(keepalive 保持 kcp 活跃)
    let mux = wrap_stream_as_server(Box::new(io), true);
    if accept_tx.try_send(mux).is_err() {
        // accept 队列满:撤销注册,客户端 kcp 重传将触发重建
        tracing::warn!("kcp accept queue full, dropping new session");
        sessions.lock().unwrap().remove(&conv);
    }
}

fn new_io() -> (KcpIo, Arc<Shared>) {
    let shared = Arc::new(Shared {
        guarded: Mutex::new(Guarded {
            read_buf: VecDeque::new(),
            read_waker: None,
            write_queue: VecDeque::new(),
            write_waker: None,
            closed: false,
        }),
        notify: Notify::new(),
    });
    (
        KcpIo {
            shared: shared.clone(),
        },
        shared,
    )
}

// ---------------- 客户端 ----------------

static CONV_COUNTER: AtomicU64 = AtomicU64::new(1);

pub async fn connect(cfg: &ClientTransportConfig) -> std::io::Result<Arc<Multiplexer>> {
    let bind_local: &str = if cfg.server_addr.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let udp = Arc::new(UdpSocket::bind(bind_local).await?);
    udp.connect(cfg.server_addr).await?;
    let conv = (CONV_COUNTER.fetch_add(1, Ordering::Relaxed) as u32) ^ rand_u32();
    let (io, shared) = new_io();
    let output = UdpOutput {
        sock: udp.clone(),
        peer: cfg.server_addr,
        connected: true,
    };
    let mut kcp = Kcp::new_stream(conv, output);
    kcp.set_mtu(MTU).ok();
    kcp.set_wndsize(256, 256);
    kcp.set_nodelay(true, 10, 2, true);
    kcp.set_maximum_resend_times(30);
    // 客户端侧接收循环:UDP → 会话任务
    let (pkt_tx, pkt_rx) = mpsc::channel::<SessionCmd>(256);
    let recv_sock = udp;
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        while let Ok((n, _)) = recv_sock.recv_from(&mut buf).await {
            if pkt_tx
                .try_send(SessionCmd::Packet(buf[..n].to_vec()))
                .is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(run_session(kcp, pkt_rx, shared));
    Ok(wrap_stream_as_client(Box::new(io), true))
}

// ---------------- NAT 打洞 ----------------

/// 打洞 listen 角色:在已绑定 socket 上被动接受首个 kcp 会话。
/// 首个 kcp 消息约定为打洞哨兵(dial 侧重传它作为打洞包),此处剥离。
pub async fn punch_listen(udp: Arc<UdpSocket>) -> std::io::Result<Arc<Multiplexer>> {
    const SENTINEL: &[u8] = b"RHTP-PUNCH!";
    // 1. 等待首个合法 kcp 包
    let mut buf = vec![0u8; 65536];
    let (n, peer) = loop {
        let (n, from) = udp.recv_from(&mut buf).await?;
        if n >= KCP_OVERHEAD && (81..=84).contains(&buf[4]) {
            break (n, from);
        }
    };
    let conv = kcp::get_conv(&buf[..n]);
    // 2. 建会话并投喂首包
    let (io, shared) = new_io();
    let output = UdpOutput {
        sock: udp.clone(),
        peer,
        connected: false,
    };
    let mut kcp = Kcp::new_stream(conv, output);
    kcp.set_mtu(MTU).ok();
    kcp.set_wndsize(256, 256);
    kcp.set_nodelay(true, 10, 2, true);
    kcp.set_maximum_resend_times(60);
    let (pkt_tx, pkt_rx) = mpsc::channel::<SessionCmd>(256);
    // 后续入站路由到会话
    tokio::spawn({
        let udp = udp.clone();
        let pkt_tx = pkt_tx.clone();
        async move {
            let mut b = vec![0u8; 65536];
            while let Ok((n, _)) = udp.recv_from(&mut b).await {
                if n >= KCP_OVERHEAD
                    && (81..=84).contains(&b[4])
                    && pkt_tx
                        .try_send(SessionCmd::Packet(b[..n].to_vec()))
                        .is_err()
                {
                    break;
                }
            }
        }
    });
    let _ = pkt_tx.try_send(SessionCmd::Packet(buf[..n].to_vec()));
    tokio::spawn(run_session(kcp, pkt_rx, shared.clone()));
    // 3. 等首条消息落地:哨兵则剥离
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        {
            let mut s = shared.guarded.lock().unwrap();
            if s.read_buf.len() >= SENTINEL.len() {
                let is_sentinel = s
                    .read_buf
                    .iter()
                    .take(SENTINEL.len())
                    .copied()
                    .collect::<Vec<u8>>()
                    == SENTINEL;
                if is_sentinel {
                    let mut rest: VecDeque<u8> = s.read_buf.split_off(SENTINEL.len());
                    std::mem::swap(&mut s.read_buf, &mut rest);
                }
                break;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    Ok(wrap_stream_as_server(Box::new(io), true))
}

/// 打洞输出:peer 可旋转(候选轮询),首包回learn 后锁定。
struct PunchOutput {
    sock: Arc<UdpSocket>,
    peer: Arc<Mutex<SocketAddr>>,
}

impl Write for PunchOutput {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let peer = *self.peer.lock().unwrap();
        match self.sock.try_send_to(buf, peer) {
            Ok(n) => Ok(n),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(buf.len()),
            Err(e) => Err(e),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 打洞 dial 角色:向全部候选轮询发送 kcp 输出与原始打洞包;
/// 首个合法 kcp 入站包锁定对端。seconds 超时返回错误。
pub async fn punch_dial(
    udp: Arc<UdpSocket>,
    candidates: &[SocketAddr],
    seconds: u64,
) -> std::io::Result<Arc<Multiplexer>> {
    if candidates.is_empty() {
        return Err(other_err("no punch candidates"));
    }
    let conv = (CONV_COUNTER.fetch_add(1, Ordering::Relaxed) as u32) ^ rand_u32();
    let peer = Arc::new(Mutex::new(candidates[0]));
    let (io, shared) = new_io();
    let mut kcp = Kcp::new_stream(
        conv,
        PunchOutput {
            sock: udp.clone(),
            peer: peer.clone(),
        },
    );
    kcp.set_mtu(MTU).ok();
    kcp.set_wndsize(256, 256);
    kcp.set_nodelay(true, 10, 2, true);
    kcp.set_maximum_resend_times(30);
    let (pkt_tx, pkt_rx) = mpsc::channel::<SessionCmd>(256);
    // 入站:合法 kcp 包 learn 对端并喂会话
    let inbound = Arc::new(tokio::sync::Notify::new());
    let learned = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let pkt_tx = pkt_tx.clone();
        let udp = udp.clone();
        let peer = peer.clone();
        let inbound = inbound.clone();
        let learned = learned.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65536];
            loop {
                let Ok((n, from)) = udp.recv_from(&mut buf).await else {
                    break;
                };
                let pkt = &buf[..n];
                if n >= KCP_OVERHEAD && (81..=84).contains(&pkt[4]) {
                    if !learned.swap(true, Ordering::Relaxed) {
                        *peer.lock().unwrap() = from;
                        inbound.notify_one();
                    }
                    if pkt_tx.try_send(SessionCmd::Packet(pkt.to_vec())).is_err() {
                        break;
                    }
                }
            }
        });
    }
    // 原始打洞包 + 候选轮询(learn 前)
    {
        let udp = udp.clone();
        let peer = peer.clone();
        let learned = learned.clone();
        let candidates = candidates.to_vec();
        tokio::spawn(async move {
            let mut i = 0usize;
            while !learned.load(Ordering::Relaxed) {
                for c in &candidates {
                    let _ = udp.try_send_to(b"RHPUNCH", *c);
                }
                *peer.lock().unwrap() = candidates[i % candidates.len()];
                i += 1;
                tokio::time::sleep(Duration::from_millis(800)).await;
            }
        });
    }
    // 打洞包 = 哨兵消息:kcp 未确认前持续重传,listen 端据其建会话并剥离。
    {
        let mut s = shared.guarded.lock().unwrap();
        s.write_queue.push_back(b"RHTP-PUNCH!".to_vec());
        drop(s);
        shared.notify.notify_one();
    }
    tokio::spawn(run_session(kcp, pkt_rx, shared));
    // 等待 learn(kcp 会话可用以开流;失败回退中继)
    let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds.max(3));
    loop {
        if learned.load(Ordering::Relaxed) {
            return Ok(wrap_stream_as_client(Box::new(io), true));
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(other_err("punch timeout"));
        }
        tokio::select! {
            _ = inbound.notified() => continue,
            _ = tokio::time::sleep(Duration::from_millis(200)) => continue,
        }
    }
}

fn rand_u32() -> u32 {
    use rand::RngCore;
    rand::rng().next_u32()
}

#[cfg(test)]
mod tests {
    use crate::{connect, listen, ClientTransportConfig, Protocol, ServerTransportConfig};
    use std::time::Duration;

    #[tokio::test]
    async fn kcp_session_streams() {
        let server_cfg = ServerTransportConfig {
            protocol: Protocol::Kcp,
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            ..Default::default()
        };
        let mut listener = listen(&server_cfg).await.unwrap();
        let addr = listener.local_addr;

        let srv = tokio::spawn(async move {
            let inc = listener.accept().await.unwrap();
            let mut s = inc.mux.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 12];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(&buf).await.unwrap();
            // 多流验证
            let mut s2 = inc.mux.accept().await.unwrap();
            let mut b2 = [0u8; 4];
            s2.read_exact(&mut b2).await.unwrap();
            s2.write_all(b"done").await.unwrap();
        });

        let cfg = ClientTransportConfig {
            protocol: Protocol::Kcp,
            server_addr: addr,
            ..Default::default()
        };
        let mux = connect(&cfg).await.unwrap();
        let mut s = mux.open_stream().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        s.write_all(b"kcp-hello-ok").await.unwrap();
        let mut back = [0u8; 12];
        s.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"kcp-hello-ok");

        let mut s2 = mux.open_stream().await.unwrap();
        s2.write_all(b"ping").await.unwrap();
        let mut b2 = [0u8; 4];
        tokio::time::timeout(Duration::from_secs(20), s2.read_exact(&mut b2))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&b2, b"done");
        srv.abort();
    }
}

#[cfg(test)]
mod punch_tests {
    use super::*;

    use std::time::Duration;

    #[tokio::test]
    async fn punch_dial_listen_loopback() {
        let listen_sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let cand = listen_sock.local_addr().unwrap();
        let dial_sock = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        let listener = tokio::spawn(async move { punch_listen(listen_sock).await });
        let dialer = tokio::spawn(async move { punch_dial(dial_sock, &[cand], 8).await });
        // 服务端先就位,接受首个流并回显
        let srv = tokio::spawn(async move {
            let mux = match listener.await {
                Ok(Ok(m)) => m,
                _ => return,
            };
            if let Ok(mut s) = mux.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut b = [0u8; 8];
                if s.read_exact(&mut b).await.is_ok() {
                    let _ = s.write_all(&b).await;
                }
            }
        });
        let mux = tokio::time::timeout(Duration::from_secs(10), dialer)
            .await
            .expect("dial join")
            .expect("dial task")
            .expect("punch dial");
        let mut s = mux.open_stream().await.expect("open");
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        s.write_all(b"punch-ok").await.unwrap();
        let mut back = [0u8; 8];
        tokio::time::timeout(Duration::from_secs(10), s.read_exact(&mut back))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&back, b"punch-ok");
        let _ = srv.await;
    }
}
