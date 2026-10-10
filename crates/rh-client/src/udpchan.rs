//! UDP 帧通道(代理侧 udp/sudp 与 visitor 侧 sudp 共用的 UDP_PACKET 帧模式)。

use rh_protocol::message::udp_packet;
use rh_protocol::{frame, FrameType, ProxySpec};
use rh_transport::BoxedIo;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

fn pong_bytes() -> Vec<u8> {
    serde_json::to_vec(&rh_protocol::message::Pong {
        ts_ms: 0,
        probe: true,
    })
    .unwrap()
}
fn ping_bytes() -> Vec<u8> {
    serde_json::to_vec(&rh_protocol::message::Ping {
        ts_ms: 0,
        probe: true,
    })
    .unwrap()
}

/// 代理侧:通道 ↔ 本地目标 UDP。按用户源地址各开一个本地 socket,
/// 回写任务经共享写半部写帧;主循环读帧 + 探活。
/// 每 60s 清理 60s 无活动的用户会话(源端口轮换的客户端会持续产生新条目)。
pub async fn run_proxy_side(channel: BoxedIo, spec: ProxySpec, prefix: Vec<u8>) {
    let target = format!("{}:{}", spec.target_host, spec.target_port);
    let Ok(target) = target.parse::<std::net::SocketAddr>() else {
        tracing::debug!(proxy = spec.name, "udp target unresolvable");
        return;
    };
    let (mut r, w) = tokio::io::split(channel);
    let w: Arc<Mutex<Option<_>>> = Arc::new(Mutex::new(Some(w)));
    let mut sessions: HashMap<std::net::SocketAddr, (Arc<UdpSocket>, Instant)> = HashMap::new();
    // hello 后已抵达的帧数据预置进缓冲
    let mut fbuf = bytes::BytesMut::from(&prefix[..]);
    let mut last_alive = Instant::now();
    let mut last_ping = Instant::now();
    let mut gc = tokio::time::interval(Duration::from_secs(60));
    gc.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let _ = gc.tick().await;
    loop {
        tokio::select! {
            rr = frame::read_frame(&mut r, &mut fbuf) => {
                match rr {
                    Ok((FrameType::UDP_PACKET, p)) => {
                        if let Some((src, data)) = udp_packet::decode(&p) {
                            if let Ok(user) = src.parse::<std::net::SocketAddr>() {
                                if let Some(s) = get_session(&mut sessions, user, target, w.clone()).await {
                                    let _ = s.send_to(data, target).await;
                                }
                            }
                            last_alive = Instant::now();
                        }
                    }
                    Ok((FrameType::PING, _)) => {
                        let guard = w.lock().await;
                        let _ww = match guard.as_ref() {
                            Some(x) => x,
                            None => break,
                        };
                        // 写需要 &mut:改为 take/write/put 回(简化:直接释放后重取)
                        drop(guard);
                        let mut g2 = w.lock().await;
                        if let Some(ww) = g2.as_mut() {
                            if frame::write_frame(ww, FrameType::PONG, &pong_bytes()).await.is_err() {
                                break;
                            }
                        } else { break; }
                        last_alive = Instant::now();
                    }
                    Ok(_) => { last_alive = Instant::now(); }
                    Err(_) => break,
                }
            }
            _ = gc.tick() => {
                let now = Instant::now();
                sessions.retain(|_, (_, t)| now.duration_since(*t) < Duration::from_secs(60));
            }
            _ = tokio::time::sleep(Duration::from_secs(15)) => {
                if last_ping.elapsed() >= Duration::from_secs(30) {
                    let mut g = w.lock().await;
                    match g.as_mut() {
                        Some(ww) => {
                            if frame::write_frame(ww, FrameType::PING, &ping_bytes()).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                    last_ping = Instant::now();
                }
                if last_alive.elapsed() > Duration::from_secs(90) {
                    break;
                }
            }
        }
    }
    let taken = w.lock().await.take();
    if let Some(mut ww) = taken {
        let _ = ww.shutdown().await;
    }
}

async fn get_session<W>(
    sessions: &mut HashMap<std::net::SocketAddr, (Arc<UdpSocket>, Instant)>,
    user: std::net::SocketAddr,
    target: std::net::SocketAddr,
    w: Arc<Mutex<Option<W>>>,
) -> Option<Arc<UdpSocket>>
where
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    if let Some((s, t)) = sessions.get_mut(&user) {
        *t = Instant::now();
        return Some(s.clone());
    }
    // 按目标地址族选择本地绑定(目标为 IPv6 时回退 IPv4 socket 必然发不出去)
    let bind_local = if target.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let sock = UdpSocket::bind(bind_local).await.ok()?;
    let sock = Arc::new(sock);
    sessions.insert(user, (sock.clone(), Instant::now()));
    // 回写任务:本地目标响应 → 通道帧(src = 该用户源地址)
    let sock2 = sock.clone();
    tokio::spawn(async move {
        let mut pkt = vec![0u8; 65536];
        loop {
            let Ok((n, _)) = sock2.recv_from(&mut pkt).await else {
                break;
            };
            let mut guard = w.lock().await;
            match guard.as_mut() {
                Some(ww) => {
                    let payload = udp_packet::encode(&user.to_string(), &pkt[..n]);
                    if frame::write_frame(ww, FrameType::UDP_PACKET, &payload)
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
                None => break,
            }
        }
    });
    Some(sock)
}

/// visitor 侧 sudp:本地 UDP 监听 ↔ 通道(src 字段承载本地用户源)。
/// 单循环:sock.recv → 帧;帧 → sock.send_to;PING/PONG 探活;
/// 结束即返回(上层重新武装)。
pub async fn run_visitor_side(
    channel: BoxedIo,
    sock: Arc<UdpSocket>,
    cancel: tokio_util::sync::CancellationToken,
    pending: Vec<(std::net::SocketAddr, Vec<u8>)>,
) {
    let (mut r, mut w) = tokio::io::split(channel);
    // 先冲通道到位前缓冲的本地包
    for (peer, data) in pending {
        let payload = udp_packet::encode(&peer.to_string(), &data);
        if frame::write_frame(&mut w, FrameType::UDP_PACKET, &payload)
            .await
            .is_err()
        {
            return;
        }
    }
    let mut fbuf = bytes::BytesMut::with_capacity(2048);
    let mut pkt = vec![0u8; 65536];
    let mut last_alive = Instant::now();
    let mut last_ping = Instant::now();
    loop {
        tokio::select! {
            rr = sock.recv_from(&mut pkt) => {
                let Ok((n, peer)) = rr else { break };
                let payload = udp_packet::encode(&peer.to_string(), &pkt[..n]);
                if frame::write_frame(&mut w, FrameType::UDP_PACKET, &payload).await.is_err() {
                    break;
                }
            }
            rr = frame::read_frame(&mut r, &mut fbuf) => {
                match rr {
                    Ok((FrameType::UDP_PACKET, p)) => {
                        if let Some((src, data)) = udp_packet::decode(&p) {
                            if let Ok(addr) = src.parse::<std::net::SocketAddr>() {
                                let _ = sock.send_to(data, addr).await;
                            }
                            last_alive = Instant::now();
                        }
                    }
                    Ok((FrameType::PING, _)) => {
                        if frame::write_frame(&mut w, FrameType::PONG, &pong_bytes()).await.is_err() {
                            break;
                        }
                        last_alive = Instant::now();
                    }
                    Ok(_) => { last_alive = Instant::now(); }
                    Err(_) => break,
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(15)) => {
                if last_ping.elapsed() >= Duration::from_secs(30) {
                    if frame::write_frame(&mut w, FrameType::PING, &ping_bytes()).await.is_err() {
                        break;
                    }
                    last_ping = Instant::now();
                }
                if last_alive.elapsed() > Duration::from_secs(90) {
                    break;
                }
            }
            _ = cancel.cancelled() => break,
        }
    }
    let _ = w.shutdown().await;
}
