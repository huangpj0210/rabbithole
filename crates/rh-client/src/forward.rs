//! START_FORWARD 处理:拨号本地目标或插件、压缩、PROXY protocol、带宽限制、
//! tcpmux passthrough、半关闭。

use crate::session::ClientSession;
use rh_core::proxyproto::{self, ProxyProtoInfo};
use rh_protocol::message::{forward_flags, StartForward};
use rh_protocol::ProxySpec;
use rh_transport::BoxedIo;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// TCP 语义数据通道:按 START_FORWARD 自包含指派桥接本地。
pub async fn handle_start_forward(
    sess: Arc<ClientSession>,
    channel: BoxedIo,
    spec: &ProxySpec,
    fwd: StartForward,
    prefix: Vec<u8>,
) -> anyhow::Result<()> {
    tracing::debug!(proxy = %fwd.proxy_name, target = %fwd.target_host, port = fwd.target_port, "forward assigned");
    // 指派帧后已抵达的用户数据:前转到本地目标(先于其他写入)
    let prefix = (!prefix.is_empty()).then_some(prefix);
    // tcpmux passthrough:通道本体是 yamux 服务端,接受多路流,每流独立转发
    if fwd.flags & forward_flags::TCPMUX_PASSTHROUGH != 0 {
        let mux = rh_transport::wrap_stream_as_server(channel, true);
        let spec = spec.clone();
        let fwd = fwd.clone();
        let sess = sess.clone();
        tokio::spawn(async move {
            while let Ok(stream) = mux.accept().await {
                let spec2 = spec.clone();
                let fwd2 = fwd.clone();
                let s2 = sess.clone();
                let pfx = prefix.clone().unwrap_or_default();
                tokio::spawn(async move {
                    if let Ok(mut local) = dial_target(&s2, &spec2, &fwd2).await {
                        if !pfx.is_empty() {
                            let _ = tokio::io::AsyncWriteExt::write_all(&mut local, &pfx).await;
                        }
                        splice(stream, local, None).await;
                    }
                });
            }
        });
        return Ok(());
    }

    let mut local = dial_target(&sess, spec, &fwd).await?;

    // PROXY protocol 头(版本以 START_FORWARD 显式指派为准)
    if fwd.proxy_proto != rh_protocol::message::ProxyProto::None {
        let info = ProxyProtoInfo {
            src: fwd
                .user_src
                .parse()
                .unwrap_or_else(|_| "127.0.0.1:0".parse().unwrap()),
            dst: "127.0.0.1:0".parse().unwrap(),
        };
        let hdr = match fwd.proxy_proto {
            rh_protocol::message::ProxyProto::V1 => proxyproto::encode_v1(&info),
            rh_protocol::message::ProxyProto::V2 => proxyproto::encode_v2(&info),
            _ => Vec::new(),
        };
        if !hdr.is_empty() && local.write_all(&hdr).await.is_err() {
            return Ok(());
        }
    }

    // 帧后残留字节(与 PROXY 头同向:通道 → 本地)
    if let Some(p) = &prefix {
        use tokio::io::AsyncWriteExt as _;
        if local.write_all(p).await.is_err() {
            return Ok(());
        }
    }

    // 压缩:端到端 snappy,两端各自 compress_stream
    let peer = if fwd.flags & forward_flags::COMPRESSED != 0 {
        rh_transport::compress_stream(channel).await
    } else {
        channel
    };

    // 带宽(客户端模式)
    let bucket = match (
        &spec.transport.bandwidth_limit,
        spec.transport.bandwidth_mode,
    ) {
        (Some(limit), rh_protocol::BandwidthMode::Client) => {
            rh_core::bandwidth::BandwidthQuantity::parse(limit).map(|q| {
                Arc::new(rh_core::ratelimit::TokenBucket::new(
                    q.bytes_per_sec() as f64
                ))
            })
        }
        _ => None,
    };

    splice(peer, local, bucket).await;
    Ok(())
}

/// 拨号本地目标(插件优先)。
pub async fn dial_target(
    _sess: &Arc<ClientSession>,
    spec: &ProxySpec,
    fwd: &StartForward,
) -> anyhow::Result<BoxedIo> {
    if let Some(plugin) = &spec.plugin {
        return crate::plugins::dial_plugin(plugin, fwd).await;
    }
    let addr = resolve(&fwd.target_host, fwd.target_port)?;
    let stream = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::net::TcpStream::connect(addr),
    )
    .await
    .map_err(|_| anyhow::anyhow!("dial {} timeout", addr))??;
    stream.set_nodelay(true).ok();
    Ok(Box::new(stream))
}

pub fn resolve(host: &str, port: u16) -> anyhow::Result<std::net::SocketAddr> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return Ok(std::net::SocketAddr::new(ip, port));
    }
    // 自定义 DNS(配置了 dns_server 时):直接向该服务器发 A/AAAA 查询
    if let Some(ns) = crate::session::dns_server() {
        if let Some(ip) = crate::dnsquery::query(host, &ns) {
            return Ok(std::net::SocketAddr::new(ip, port));
        }
    }
    use std::net::ToSocketAddrs;
    let mut addrs = (host, port).to_socket_addrs()?;
    addrs
        .next()
        .ok_or_else(|| anyhow::anyhow!("resolve {host}:{port} failed"))
}

/// 双向搬运(保持半关闭:一边 EOF 只关闭另一边的写方向)。
pub async fn splice(a: BoxedIo, b: BoxedIo, bucket: Option<Arc<rh_core::ratelimit::TokenBucket>>) {
    let (mut ar, mut aw) = tokio::io::split(a);
    let (mut br, mut bw) = tokio::io::split(b);
    let bucket2 = bucket.clone();
    let t1 = tokio::spawn(async move {
        copy_limited(&mut ar, &mut bw, bucket).await;
    });
    let t2 = tokio::spawn(async move {
        copy_limited(&mut br, &mut aw, bucket2).await;
    });
    let _ = t1.await;
    let _ = t2.await;
}

/// 带限速的拷贝;EOF 时对写端做 shutdown(半关闭传递)。
pub async fn copy_limited<R, W>(
    r: &mut R,
    w: &mut W,
    bucket: Option<Arc<rh_core::ratelimit::TokenBucket>>,
) where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match r.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if let Some(b) = &bucket {
                    b.acquire(n).await;
                }
                if w.write_all(&buf[..n]).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = w.shutdown().await;
}
