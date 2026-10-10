//! 数据面:TCP 转发(池/取一补一/自包含指派)与 UDP 通道(服务端负责制)。
//! vhost http/https/tcpmux 与 visitor 中继复用 get_channel + assign 指派。

use crate::session::{RequestPurpose, Session};
use crate::state::{AppState, ProxyRuntime, TcpPool};
use rh_core::ratelimit::TokenBucket;
use rh_protocol::message::udp_packet;
use rh_protocol::message::{ChannelEntity, StartForward};
use rh_protocol::{forward_flags, FrameType, ProxySpec};
use rh_transport::{compress_stream, BoxedIo};
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::mpsc;
use tracing::{debug, warn};

// ---------------- 绑定 ----------------

pub async fn bind_tcp_listener(
    _state: &Arc<AppState>,
    spec: &ProxySpec,
    bind_ip: std::net::IpAddr,
) -> std::io::Result<TcpListener> {
    let addr = SocketAddr::new(bind_ip, spec.bind_port);
    TcpListener::bind(addr).await
}

pub async fn bind_udp_socket(
    _state: &Arc<AppState>,
    spec: &ProxySpec,
    bind_ip: std::net::IpAddr,
) -> std::io::Result<Arc<UdpSocket>> {
    let addr = SocketAddr::new(bind_ip, spec.bind_port);
    let std_sock = std::net::UdpSocket::bind(addr)?;
    std_sock.set_nonblocking(true)?;
    Ok(Arc::new(UdpSocket::from_std(std_sock)?))
}

pub async fn close_proxy_channels(rt: &Arc<ProxyRuntime>) {
    // 池与通道由取消/关闭级联;此处显式清空池。
    if let Some(p) = rt.pool.lock().await.as_ref() {
        let mut ready = p.ready.lock().await;
        ready.clear();
    }
    *rt.udp_new_channel_tx.lock().await = None;
}

// ---------------- TCP 池 ----------------

/// 会话认领池(epoch 全局单调,仅允许向前):驱逐旧会话遗留的就绪条目。
/// 会话重连/替换后,旧会话建立的通道对端已不可用,必须整体失效。
async fn claim_pool(pool: &Arc<TcpPool>, sess: &Arc<Session>) {
    let cur = pool.epoch.load(Ordering::Relaxed);
    if sess.epoch > cur {
        pool.epoch.store(sess.epoch, Ordering::Relaxed);
        let mut ready = pool.ready.lock().await;
        let mut keep = Vec::with_capacity(ready.len());
        for entry in ready.drain(..) {
            if entry.0 == sess.epoch {
                keep.push(entry);
            }
        }
        *ready = keep; // 旧会话条目在此丢弃,级联关闭通道
    }
}

/// 池水位维持(取一补一):池中+在途 < pool_size 时签发 CREATE。
pub async fn tcp_refill(state: &Arc<AppState>, sess: &Arc<Session>, rt: &Arc<ProxyRuntime>) {
    let Some(pool) = rt.pool.lock().await.clone() else {
        return;
    };
    claim_pool(&pool, sess).await;
    let ready = pool.ready.lock().await.len() as u64;
    let inflight = pool.inflight.load(Ordering::Relaxed);
    let want = pool.size as u64;
    if ready + inflight >= want {
        return;
    }
    let n = want - ready - inflight;
    for _ in 0..n {
        pool.inflight.fetch_add(1, Ordering::Relaxed);
        match state
            .issue_create_channel(
                sess,
                ChannelEntity::Proxy { proxy_id: rt.id },
                RequestPurpose::TcpPool { proxy_id: rt.id },
            )
            .await
        {
            Some(_rid) => {}
            None => {
                pool.inflight.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }
    }
}

/// 数据通道抵达:入池或直接交付等待者。
pub async fn tcp_on_channel(
    state: &Arc<AppState>,
    sess: &Arc<Session>,
    proxy_id: u64,
    request_id: u64,
    stream: BoxedIo,
) {
    let Some(rt) = state.proxies.get(proxy_id) else {
        return;
    };
    let Some(pool) = rt.pool.lock().await.clone() else {
        return;
    };
    pool.inflight.fetch_sub(1, Ordering::Relaxed);
    // 旧会话晚到的通道(会话已替换/重连):直接丢弃
    let owner = pool.epoch.load(Ordering::Relaxed);
    if owner != 0 && owner != sess.epoch {
        return;
    }
    // 有等待者直接交付;等待者已放弃(超时)则通道入池,不得丢弃
    let deliver = {
        let mut w = pool.waiters.lock().await;
        w.pop()
    };
    match deliver {
        Some((_, tx)) => {
            if let Err(stream) = tx.send(stream) {
                // 接收端已放弃:通道回到池或关闭
                let mut ready = pool.ready.lock().await;
                if ready.len() < pool.size {
                    ready.push((sess.epoch, request_id, stream));
                } else {
                    drop(stream);
                }
            }
        }
        None => {
            let mut ready = pool.ready.lock().await;
            if ready.len() < pool.size {
                ready.push((sess.epoch, request_id, stream));
            } else {
                // 超额:关闭
                drop(stream);
            }
        }
    }
    // 取一补一
    tcp_refill(state, sess, &rt).await;
}

/// 获取一条到客户端的数据通道(≤ pool_wait_timeout)。
pub async fn get_channel(state: &Arc<AppState>, rt: &Arc<ProxyRuntime>) -> Option<BoxedIo> {
    let sess = state.sessions.get(&rt.client_id)?;
    let pool = rt.pool.lock().await.clone()?;
    claim_pool(&pool, &sess).await;
    // 就绪直取(跳过旧会话遗留条目)
    {
        let mut ready = pool.ready.lock().await;
        let owner = pool.epoch.load(Ordering::Relaxed);
        while let Some((epoch, _rid, s)) = ready.pop() {
            if epoch == owner {
                // 注意:必须先释放 ready 锁再 refill(内部会再取同锁)
                drop(ready);
                tcp_refill(state, &sess, rt).await;
                return Some(s);
            }
            // 旧会话条目:丢弃继续
        }
    }
    // 等待新通道(签发后到达即交付)
    let (tx, rx) = tokio::sync::oneshot::channel::<BoxedIo>();
    let waiter_id = pool.waiter_seq.fetch_add(1, Ordering::Relaxed);
    pool.waiters.lock().await.push((waiter_id, tx));
    // 签发
    pool.inflight.fetch_add(1, Ordering::Relaxed);
    let issued = state
        .issue_create_channel(
            &sess,
            ChannelEntity::Proxy { proxy_id: rt.id },
            RequestPurpose::TcpPool { proxy_id: rt.id },
        )
        .await
        .is_some();
    if !issued {
        pool.inflight.fetch_sub(1, Ordering::Relaxed);
        pool.remove_waiter(waiter_id).await;
        return None;
    }
    match tokio::time::timeout(
        Duration::from_millis(state.cfg.limits.pool_wait_timeout_ms),
        rx,
    )
    .await
    {
        Ok(Ok(s)) => Some(s),
        _ => {
            pool.remove_waiter(waiter_id).await;
            None
        }
    }
}

/// START_FORWARD 指派(数据连接最后一帧)+ 桥接到用户连接。
/// 半关闭:双向独立复制。
pub async fn assign_forward(
    _state: &Arc<AppState>,
    rt: &Arc<ProxyRuntime>,
    mut channel: BoxedIo,
    user: BoxedIo,
    user_src: &str,
    local_addr_override: Option<SocketAddr>,
) {
    let spec = rt.spec.read().await.clone();
    let target_host = spec.target_host.clone();
    let target_port = spec.target_port;
    let mut flags: u8 = 0;
    if spec.transport.use_compression {
        flags |= forward_flags::COMPRESSED;
    }
    let fwd = StartForward {
        proxy_id: rt.id,
        user_src: user_src.to_string(),
        target_host,
        target_port,
        flags,
        proxy_proto: conv_proxy_proto(spec.transport.proxy_proto),
        proxy_name: spec.name.clone(),
    };
    let payload = match serde_json::to_vec(&fwd) {
        Ok(p) => p,
        Err(_) => return,
    };
    if rh_protocol::write_frame(&mut channel, FrameType::START_FORWARD, &payload)
        .await
        .is_err()
    {
        return; // 死连接:上层取下一条/重试
    }
    let channel: BoxedIo = if spec.transport.use_compression {
        compress_stream(channel).await
    } else {
        channel
    };
    // 带宽限制(server 模式)
    let bucket = spec
        .transport
        .bandwidth_limit
        .as_ref()
        .and_then(|s| rh_core::BandwidthQuantity::parse(s))
        .filter(|_| {
            matches!(
                spec.transport.bandwidth_mode,
                rh_protocol::BandwidthMode::Server
            )
        })
        .map(|q| Arc::new(TokenBucket::new(q.bytes_per_sec() as f64)));
    let (mut u_r, mut u_w) = tokio::io::split(user);
    let (mut c_r, mut c_w) = tokio::io::split(channel);
    let _ = local_addr_override;
    let bucket2 = bucket.clone();
    let cancel = rt.rule_cancel.clone();
    let t1 = tokio::spawn(async move {
        // 规则取消(禁用/删除)级联关闭在途转发(§4.1)
        tokio::select! {
            _ = copy_limited(&mut u_r, &mut c_w, bucket2) => {}
            _ = cancel.cancelled() => {}
        }
        let _ = c_w.shutdown().await;
    });
    let cancel2 = rt.rule_cancel.clone();
    let t2 = tokio::spawn(async move {
        tokio::select! {
            _ = copy_limited(&mut c_r, &mut u_w, bucket) => {}
            _ = cancel2.cancelled() => {}
        }
        let _ = u_w.shutdown().await;
    });
    let _ = tokio::join!(t1, t2);
}

async fn copy_limited<R, W>(r: &mut R, w: &mut W, bucket: Option<Arc<TokenBucket>>)
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buf = vec![0u8; 32 * 1024];
    loop {
        let n = match r.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => n,
            Err(_) => break,
        };
        if let Some(b) = &bucket {
            b.acquire(n).await;
        }
        if w.write_all(&buf[..n]).await.is_err() {
            break;
        }
    }
}

/// TCP 监听服务循环(独享端口或组成员共享)。
pub async fn tcp_serve_loop(state: Arc<AppState>, rt: Arc<ProxyRuntime>, listener: TcpListener) {
    let local = listener.local_addr().ok();
    loop {
        tokio::select! {
            acc = listener.accept() => {
                match acc {
                    Ok((sock, peer)) => {
                                        let st = state.clone();
                        let rt2 = rt.clone();
                        tokio::spawn(serve_tcp_user(st, rt2, sock, peer, local));
                    }
                    Err(e) => {
                        warn!(error = %e, "tcp accept error");
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                }
            }
            _ = rt.rule_cancel.cancelled() => return,
        }
    }
}

/// 连接计数守卫:转发结束(含指派失败的早退路径)即递减,
/// 避免 conns_current 在死通道场景下单调漂移。
struct ConnGuard(Arc<crate::state::Metrics>);

impl Drop for ConnGuard {
    fn drop(&mut self) {
        self.0.totals.lock().unwrap().conns_current -= 1;
    }
}

async fn serve_tcp_user(
    state: Arc<AppState>,
    rt: Arc<ProxyRuntime>,
    sock: TcpStream,
    peer: SocketAddr,
    local: Option<SocketAddr>,
) {
    let _ = sock.set_nodelay(true);
    if !rt.healthy.load(Ordering::Relaxed) {
        return; // 健康摘除:快速失败
    }
    // 组选路
    let target_rt = match group_pick(&state, &rt).await {
        Some(t) => t,
        None => rt,
    };
    let Some(channel) = get_channel(&state, &target_rt).await else {
        return; // 池空超时:快速失败
    };
    state.metrics.totals.lock().unwrap().conns_current += 1;
    let _conn_guard = ConnGuard(state.metrics.clone());
    state
        .metrics
        .connections
        .with_label_values(&[&target_rt.id.to_string(), "tcp"])
        .inc();
    assign_forward(
        &state,
        &target_rt,
        channel,
        Box::new(sock),
        &peer.to_string(),
        local,
    )
    .await;
    state
        .metrics
        .connections
        .with_label_values(&[&target_rt.id.to_string(), "tcp"])
        .dec();
}

/// 组内健康成员选取(公开:vhost 等复用)。
pub async fn group_pick_public(
    state: &Arc<AppState>,
    rt: &Arc<ProxyRuntime>,
) -> Option<Arc<ProxyRuntime>> {
    group_pick(state, rt).await
}

/// 组内健康成员选取(组键由 spec 决定)。
async fn group_pick(state: &Arc<AppState>, rt: &Arc<ProxyRuntime>) -> Option<Arc<ProxyRuntime>> {
    let key = {
        let spec = rt.spec.read().await;
        crate::state::ProxyManager::group_key(&spec)?
    };
    let (members, start) = {
        let g = state.proxies.groups.get(&key)?;
        let members = g.members.read().await.clone();
        let start = g.rr.fetch_add(1, Ordering::Relaxed) as usize;
        (members, start)
    };
    if members.len() <= 1 {
        return Some(rt.clone());
    }
    // 轮询健康且在线的成员
    for idx in (start..).take(members.len()) {
        let id = members[idx % members.len()];
        if let Some(m) = state.proxies.get(id) {
            if m.healthy.load(Ordering::Relaxed) && state.sessions.get(&m.client_id).is_some() {
                return Some(m);
            }
        }
    }
    None
}

// ---------------- UDP 通道(服务端负责制) ----------------

/// 建立/重建 UDP 通道(每规则至多 1 个在途请求)。
pub async fn udp_ensure_channel(
    state: &Arc<AppState>,
    sess: &Arc<Session>,
    rt: &Arc<ProxyRuntime>,
) {
    // 在途判断:pending_requests 中已有该规则的 UdpChannel 即等待
    let has_pending = {
        let m = sess.pending_requests.lock().await;
        m.values().any(
            |p| matches!(p.purpose, RequestPurpose::UdpChannel { proxy_id } if proxy_id == rt.id),
        )
    };
    if has_pending {
        return;
    }
    let _ = state
        .issue_create_channel(
            sess,
            ChannelEntity::Proxy { proxy_id: rt.id },
            RequestPurpose::UdpChannel { proxy_id: rt.id },
        )
        .await;
}

/// UDP 通道抵达:交给该规则的 pump 任务。
pub async fn udp_on_channel(
    state: &Arc<AppState>,
    _sess: &Arc<Session>,
    proxy_id: u64,
    stream: BoxedIo,
) {
    let Some(rt) = state.proxies.get(proxy_id) else {
        return;
    };
    // 克隆 sender 后立即放锁:send 可能因 pump 忙而阻塞,
    // 不得让锁跨 await(close_proxy_channels 会被卡死)
    let tx = rt.udp_new_channel_tx.lock().await.clone();
    if let Some(tx) = tx {
        let _ = tx.send(stream).await;
    }
}

/// UDP 服务主循环:等待通道 → 双向搬运;死亡 → 退避重建。
pub async fn udp_serve_loop(state: Arc<AppState>, rt: Arc<ProxyRuntime>, sock: Arc<UdpSocket>) {
    let (new_tx, mut new_rx) = mpsc::channel::<BoxedIo>(4);
    *rt.udp_new_channel_tx.lock().await = Some(new_tx);
    let mut sessions: std::collections::HashMap<SocketAddr, Instant> =
        std::collections::HashMap::new();
    let mut gc = tokio::time::interval(Duration::from_secs(15));
    loop {
        // 等待第一条/重建通道
        let stream = tokio::select! {
            s = new_rx.recv() => match s { Some(s) => s, None => return },
            _ = rt.rule_cancel.cancelled() => return,
            _ = state.shutdown.cancelled() => return,
        };
        let (mut ch_r, mut ch_w) = tokio::io::split(stream);
        let mut frame_buf = bytes::BytesMut::with_capacity(2048);
        let mut pkt_buf = vec![0u8; state.cfg.limits.udp_packet_size.max(1500)];
        let mut last_ping_sent = Instant::now();
        let mut last_alive = Instant::now();
        let dead = loop {
            tokio::select! {
                r = sock.recv_from(&mut pkt_buf) => {
                    match r {
                        Ok((n, peer)) => {
                            let payload = udp_packet::encode(&peer.to_string(), &pkt_buf[..n]);
                            if rh_protocol::write_frame(&mut ch_w, FrameType::UDP_PACKET, &payload).await.is_err() {
                                break true;
                            }
                            sessions.insert(peer, Instant::now());
                        }
                        Err(_) => break false, // 监听 socket 失效:整个规则退出
                    }
                }
                r = rh_protocol::read_frame(&mut ch_r, &mut frame_buf) => {
                    match r {
                        Ok((FrameType::UDP_PACKET, p)) => {
                            if let Some((src, data)) = udp_packet::decode(&p) {
                                if let Ok(addr) = src.parse::<SocketAddr>() {
                                    let _ = sock.send_to(data, addr).await;
                                    sessions.insert(addr, Instant::now());
                                }
                                last_alive = Instant::now();
                            }
                        }
                        Ok((FrameType::PING, _)) => {
                            let pong = serde_json::to_vec(&rh_protocol::message::Pong { ts_ms: 0, probe: true }).unwrap();
                            if rh_protocol::write_frame(&mut ch_w, FrameType::PONG, &pong).await.is_err() {
                                break true;
                            }
                            last_alive = Instant::now();
                        }
                        Ok(_) => { last_alive = Instant::now(); }
                        Err(_) => break true, // EOF/错误:通道死亡
                    }
                }
                _ = gc.tick() => {
                    // 会话表淘汰必须在通道存活期间执行(不能只等通道死亡),
                    // 否则源地址轮换的 UDP 对端会无界积累
                    let now = Instant::now();
                    sessions.retain(|_, t| now.duration_since(*t) < Duration::from_secs(60));
                }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    let now = Instant::now();
                    if now.duration_since(last_ping_sent) >= Duration::from_secs(30) {
                        let ping = serde_json::to_vec(&rh_protocol::message::Ping { ts_ms: 0, probe: true }).unwrap();
                        if rh_protocol::write_frame(&mut ch_w, FrameType::PING, &ping).await.is_err() {
                            break true;
                        }
                        last_ping_sent = now;
                    }
                    if now.duration_since(last_alive) > Duration::from_secs(90) {
                        break true; // 探活超时
                    }
                }
                _ = rt.rule_cancel.cancelled() => return,
                _ = state.shutdown.cancelled() => return,
            }
        };
        if dead {
            debug!(proxy = rt.id, "udp channel dead");
            if let Some(sess) = state.sessions.get(&rt.client_id) {
                // 每次通道重建固定退避 1s(原指数值在下次通道到达即被重置,等效恒为 1)
                tokio::time::sleep(Duration::from_secs(1)).await;
                udp_ensure_channel(&state, &sess, &rt).await;
            }
            // 清用户会话映射(代次隔离)
            sessions.clear();
        }
    }
}

fn conv_proxy_proto(v: rh_protocol::ProxyProtoVersion) -> rh_protocol::message::ProxyProto {
    match v {
        rh_protocol::ProxyProtoVersion::None => rh_protocol::message::ProxyProto::None,
        rh_protocol::ProxyProtoVersion::V1 => rh_protocol::message::ProxyProto::V1,
        rh_protocol::ProxyProtoVersion::V2 => rh_protocol::message::ProxyProto::V2,
    }
}
