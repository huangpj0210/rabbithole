//! 客户端会话:认证、配置同步应用、控制读写循环、数据通道建立。

use crate::config::ClientConfig;
use rh_protocol::message::{
    ApplyResult, ChannelEntity, ClientMessage, CreateDataChannel, DataChannelHello, Hello,
    ServerMessage, StartForward, StartVisitorRelay,
};
use rh_protocol::{frame, FrameType, ProxyKind, ProxySpec, VisitorSpec};
use rh_transport::BoxedIo;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

/// 一次已建立会话的全部运行态。
pub struct ClientSession {
    pub cfg: Arc<ClientConfig>,
    pub session_id: String,
    pub heartbeat_s: u32,
    /// 控制写队列。
    pub send_tx: mpsc::Sender<(FrameType, Vec<u8>)>,
    pub cancel: CancellationToken,
    // 实体表
    pub proxies: Mutex<HashMap<u64, ProxySpec>>,
    pub visitors: Mutex<HashMap<u64, VisitorSpec>>,
    /// visitor 监听任务取消器。
    pub visitor_cancels: Mutex<HashMap<u64, CancellationToken>>,
    /// 健康检查任务取消器。
    pub health_cancels: Mutex<HashMap<u64, CancellationToken>>,
    /// stcp/xtcp 等待配对的本地连接:user_conn_id → (连接本体, visitor_id)。
    pub waiting: Mutex<HashMap<u64, (tokio::net::TcpStream, u64)>>,
    /// XTCP 打洞成功后的直连多路复用:visitor_id → mux。
    pub punched: Mutex<HashMap<u64, Arc<rh_transport::Multiplexer>>>,
    /// 打洞事务保留 socket(候选与打洞同源;双端各自入队)。
    pub nat_socks: Mutex<HashMap<String, std::collections::VecDeque<Arc<tokio::net::UdpSocket>>>>,
    /// sudp 通道交付:visitor_id → sender。
    pub sudp_deliver: Mutex<HashMap<u64, mpsc::Sender<BoxedIo>>>,
    pub mux: Arc<rh_transport::Multiplexer>,
    pub conn_counter: AtomicU64,
}

/// 在真实流之前回放一段残留字节(read_frame 缓冲丢失问题)。
pub use rh_transport::PrependedIo;

/// 当前自定义 DNS 服务器(见 dnsquery)。
pub fn dns_server() -> Option<String> {
    crate::dnsquery::server()
}

pub fn frame_type_of_client(m: &ClientMessage) -> FrameType {
    match m {
        ClientMessage::Hello(_) => FrameType::HELLO,
        ClientMessage::AuthResponse(_) => FrameType::AUTH_RESPONSE,
        ClientMessage::Ping(_) => FrameType::PING,
        ClientMessage::SyncRequest {} => FrameType::SYNC_REQUEST,
        ClientMessage::DataChannelHello(_) => FrameType::DATA_CHANNEL_HELLO,
        ClientMessage::ApplyResult(_) => FrameType::APPLY_RESULT,
        ClientMessage::UserConnect(_) => FrameType::USER_CONNECT,
        ClientMessage::HealthReport(_) => FrameType::HEALTH_REPORT,
        ClientMessage::NatHoleReport(_) => FrameType::NAT_HOLE_REPORT,
    }
}

impl ClientSession {
    pub async fn send_msg(&self, m: ClientMessage) -> bool {
        let payload = serde_json::to_vec(&m).unwrap_or_default();
        self.send_tx
            .send((frame_type_of_client(&m), payload))
            .await
            .is_ok()
    }

    pub async fn next_conn_id(&self) -> u64 {
        self.conn_counter.fetch_add(1, Ordering::Relaxed) + 1
    }
}

/// 建立:连接 → 控制流 → HELLO/挑战/应答 → 运行会话循环。
/// Err 返回触发外层重连。
pub async fn connect_and_run(
    cfg: Arc<ClientConfig>,
    shutdown: CancellationToken,
) -> anyhow::Result<()> {
    let tcfg = cfg.transport_cfg()?;
    let mux = rh_transport::connect(&tcfg).await?;
    let mut control = mux.open_stream().await?;
    // HELLO
    let hostname = hostname_or_unknown();
    let hello = Hello {
        client_id: cfg.client_id.clone(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        hostname,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        capabilities: vec![
            "tcp", "udp", "http", "https", "stcp", "sudp", "xtcp", "tcpmux",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    };
    frame::write_frame(&mut control, FrameType::HELLO, &serde_json::to_vec(&hello)?).await?;
    let mut buf = bytes::BytesMut::with_capacity(1024);
    // 挑战或错误
    let (ty, payload) = tokio::time::timeout(
        Duration::from_secs(10),
        frame::read_frame(&mut control, &mut buf),
    )
    .await
    .map_err(|_| anyhow::anyhow!("hello timeout"))??;
    match ty {
        FrameType::ERROR => {
            let e: ServerMessage = serde_json::from_slice(&payload).unwrap_or(
                ServerMessage::Error(rh_protocol::message::ErrorMessage {
                    code: "UNKNOWN".into(),
                    message: String::from_utf8_lossy(&payload).into_owned(),
                }),
            );
            let (code, msg) = match e {
                ServerMessage::Error(e) => (e.code, e.message),
                _ => ("UNKNOWN".to_string(), "unexpected".to_string()),
            };
            anyhow::bail!("server rejected: {code}: {msg}");
        }
        FrameType::AUTH_CHALLENGE => {
            let ch: rh_protocol::message::AuthChallenge = serde_json::from_slice(&payload)?;
            let auth = hmac_hex(
                cfg.secret.as_bytes(),
                &format!("rh2:{}:{}", cfg.client_id, ch.nonce),
            );
            // 裸帧携带 AuthResponse 本体(非 ClientMessage 信封)
            let resp = rh_protocol::message::AuthResponse {
                client_id: cfg.client_id.clone(),
                auth,
            };
            frame::write_frame(
                &mut control,
                FrameType::AUTH_RESPONSE,
                &serde_json::to_vec(&resp)?,
            )
            .await?;
        }
        other => anyhow::bail!("unexpected first frame {other:?}"),
    }
    // HELLO_ACK(或错误)
    let (ty, payload) = tokio::time::timeout(
        Duration::from_secs(10),
        frame::read_frame(&mut control, &mut buf),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ack timeout"))??;
    if ty == FrameType::ERROR {
        let e: rh_protocol::message::ErrorMessage = serde_json::from_slice(&payload)
            .map_err(|_| anyhow::anyhow!("server error (unparsable)"))?;
        anyhow::bail!("server rejected: {}: {}", e.code, e.message);
    }
    if ty != FrameType::HELLO_ACK {
        anyhow::bail!("expected HELLO_ACK, got {ty:?}");
    }
    let ack: rh_protocol::message::HelloAck = serde_json::from_slice(&payload)?;
    if !ack.ok {
        anyhow::bail!("hello rejected: {:?}", ack.error_code);
    }
    let session_id = ack.session_id.clone().unwrap_or_default();
    info!(server = %cfg.server_addr, session = %session_id, "connected");

    let (send_tx, send_rx) = mpsc::channel::<(FrameType, Vec<u8>)>(256);
    let sess = Arc::new(ClientSession {
        cfg: cfg.clone(),
        session_id: session_id.clone(),
        heartbeat_s: ack.heartbeat_interval_s.max(10),
        send_tx,
        cancel: CancellationToken::new(),
        proxies: Mutex::new(HashMap::new()),
        visitors: Mutex::new(HashMap::new()),
        visitor_cancels: Mutex::new(HashMap::new()),
        health_cancels: Mutex::new(HashMap::new()),
        waiting: Mutex::new(HashMap::new()),
        punched: Mutex::new(HashMap::new()),
        nat_socks: Mutex::new(HashMap::new()),
        sudp_deliver: Mutex::new(HashMap::new()),
        mux: mux.clone(),
        conn_counter: AtomicU64::new(0),
    });
    let _ = shutdown.clone();
    run_loop(sess.clone(), control, send_rx, shutdown).await;
    sess.cancel.cancel();
    Ok(())
}

fn hostname_or_unknown() -> String {
    #[cfg(unix)]
    {
        if let Ok(n) = std::fs::read_to_string("/proc/sys/kernel/hostname") {
            return n.trim().to_string();
        }
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".into())
}

pub fn hmac_hex(secret: &[u8], msg: &str) -> String {
    use hmac::Mac;
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret).expect("hmac key");
    mac.update(msg.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

// ---------------- 主循环 ----------------

async fn run_loop(
    sess: Arc<ClientSession>,
    mut control: BoxedIo,
    mut send_rx: mpsc::Receiver<(FrameType, Vec<u8>)>,
    shutdown: CancellationToken,
) {
    let mut buf = bytes::BytesMut::with_capacity(4096);
    let mut hb = tokio::time::interval(Duration::from_secs(sess.heartbeat_s as u64));
    hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let _ = hb.tick().await;
    // 活性看门狗:死链(NAT 老化/黑洞路由)下 TCP 写要 15-20 分钟才报错,
    // 3× 心跳周期无任何服务端帧即主动断开触发重连(与服务端 90s 看门狗对称)
    let mut last_rx = std::time::Instant::now();
    let watchdog = Duration::from_secs(sess.heartbeat_s as u64 * 3);
    loop {
        tokio::select! {
            r = frame::read_frame(&mut control, &mut buf) => {
                match r {
                    Ok((ty, payload)) => {
                        last_rx = std::time::Instant::now();
                        if let Err(e) = dispatch_frame(&sess, &mut control, &mut buf, ty, &payload, &shutdown).await {
                            if e.to_string().starts_with("session rejected") {
                                sess.cancel.cancel();
                            }
                            warn!(error = %e, "control dispatch error");
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "control read error");
                        break;
                    }
                }
            }
            out = send_rx.recv() => {
                match out {
                    Some((ty, payload)) => {
                        if frame::write_frame(&mut control, ty, &payload).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            _ = hb.tick() => {
                if last_rx.elapsed() > watchdog {
                    warn!("control link watchdog: no server frames; reconnecting");
                    break;
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                if !sess
                    .send_msg(ClientMessage::Ping(rh_protocol::message::Ping {
                        ts_ms: now,
                        probe: false,
                    }))
                    .await
                {
                    break;
                }
            }
            _ = sess.cancel.cancelled() => {
                let _ = control.shutdown().await;
                break;
            }
            _ = shutdown.cancelled() => {
                let _ = control.shutdown().await;
                break;
            }
        }
    }
    // 会话结束:停全部实体任务
    let ids: Vec<u64> = sess.visitor_cancels.lock().await.keys().copied().collect();
    for id in ids {
        stop_visitor(&sess, id).await;
    }
    let ids: Vec<u64> = sess.health_cancels.lock().await.keys().copied().collect();
    for id in ids {
        stop_proxy(&sess, id).await;
    }
}

/// 供 main 循环调用的读分发(见 run_session)。
pub async fn dispatch_frame(
    sess: &Arc<ClientSession>,
    control: &mut BoxedIo,
    buf: &mut bytes::BytesMut,
    ty: FrameType,
    payload: &[u8],
    shutdown: &CancellationToken,
) -> anyhow::Result<()> {
    // 控制流消息经会话队列下发,均为 ServerMessage 信封;
    // 裸帧(握手/数据通道指派)另见 open_data_channel。
    if matches!(
        ty,
        FrameType::SERVICE_SYNC
            | FrameType::SERVICE_UPDATE
            | FrameType::SERVICE_REMOVE
            | FrameType::CREATE_DATA_CHANNEL
            | FrameType::NAT_HOLE_PREPARE
            | FrameType::NAT_HOLE_CONNECT
            | FrameType::ERROR
    ) {
        let m: ServerMessage = serde_json::from_slice(payload)?;
        match m {
            ServerMessage::ServiceSync(s) => {
                apply_sync(sess, s.revision, s.entities).await;
                ack(sess, s.revision).await;
            }
            ServerMessage::ServiceUpdate(u) => {
                upsert(sess, u.entity).await;
                ack(sess, u.revision).await;
            }
            ServerMessage::ServiceRemove(r) => {
                remove_entity(sess, &r.entity_type, r.entity_id).await;
                ack(sess, r.revision).await;
            }
            ServerMessage::CreateDataChannel(c) => {
                tracing::debug!(request_id = c.request_id, entity = ?c.entity, "create data channel received");
                let sess2 = sess.clone();
                tokio::spawn(async move {
                    if let Err(e) = open_data_channel(sess2, c).await {
                        warn!(error = %e, "data channel failed");
                    }
                });
            }
            ServerMessage::NatHolePrepare(p) => {
                crate::nathole_cli::on_prepare(sess.clone(), p).await;
            }
            ServerMessage::NatHoleConnect(c) => {
                let s2 = sess.clone();
                let sd = shutdown.clone();
                tokio::spawn(async move {
                    crate::nathole_cli::on_connect(s2, c, sd).await;
                });
            }
            ServerMessage::Error(e) => {
                warn!(code = %e.code, msg = %e.message, "server error frame");
                if e.code == "AUTH_FAILED" || e.code == "CLIENT_DISABLED" {
                    return Err(anyhow::anyhow!("session rejected: {}", e.code));
                }
            }
            _ => warn!("unexpected envelope frame {ty:?}"),
        }
        return Ok(());
    }
    match ty {
        FrameType::PING => {
            let p: rh_protocol::message::Ping =
                serde_json::from_slice(payload).unwrap_or(rh_protocol::message::Ping {
                    ts_ms: 0,
                    probe: false,
                });
            let _ = sess
                .send_msg(ClientMessage::Ping(rh_protocol::message::Ping {
                    ts_ms: p.ts_ms,
                    probe: false,
                }))
                .await;
        }
        FrameType::PONG => {}
        other => {
            let _ = (control, buf);
            warn!(frame = ?other, "ignored frame");
        }
    }
    Ok(())
}

async fn ack(sess: &Arc<ClientSession>, revision: u64) {
    let _ = sess
        .send_msg(ClientMessage::ApplyResult(ApplyResult {
            revision,
            ok: true,
            error: None,
        }))
        .await;
}

// ---------------- 同步应用 ----------------

async fn apply_sync(
    sess: &Arc<ClientSession>,
    revision: u64,
    entities: Vec<rh_protocol::message::SyncEntity>,
) {
    // 幂等全量替换:停掉不在新集合/已变更的,启动新增/变更的
    let mut new_proxies: HashMap<u64, ProxySpec> = HashMap::new();
    let mut new_visitors: HashMap<u64, VisitorSpec> = HashMap::new();
    for e in entities {
        match e {
            rh_protocol::message::SyncEntity::Proxy(p) => {
                new_proxies.insert(p.id, *p);
            }
            rh_protocol::message::SyncEntity::Visitor(v) => {
                new_visitors.insert(v.id, v);
            }
        }
    }
    // proxies
    {
        let mut cur = sess.proxies.lock().await;
        let old: Vec<(u64, ProxySpec)> = cur.drain().collect();
        for (id, old_spec) in old {
            match new_proxies.get(&id) {
                Some(new) if *new == old_spec => {
                    cur.insert(id, old_spec);
                }
                _ => stop_proxy(sess, id).await,
            }
        }
        let existing: Vec<u64> = cur.keys().copied().collect();
        for (id, spec) in &new_proxies {
            if existing.contains(id) {
                continue;
            }
            let mut spec = spec.clone();
            spec.normalize_kind_configs();
            cur.insert(*id, spec.clone());
            start_health(sess, *id, &spec).await;
        }
    }
    // visitors
    {
        let mut cur = sess.visitors.lock().await;
        let old: Vec<(u64, VisitorSpec)> = cur.drain().collect();
        for (id, old_spec) in old {
            match new_visitors.get(&id) {
                Some(new) if *new == old_spec => {
                    cur.insert(id, old_spec);
                }
                _ => stop_visitor(sess, id).await,
            }
        }
        let existing: Vec<u64> = cur.keys().copied().collect();
        for (id, spec) in &new_visitors {
            if existing.contains(id) {
                continue;
            }
            let spec = spec.clone();
            cur.insert(*id, spec.clone());
            crate::visitor_run::start(sess.clone(), *id, spec).await;
        }
    }
    info!(revision, "sync applied");
}

async fn upsert(sess: &Arc<ClientSession>, e: rh_protocol::message::SyncEntity) {
    match e {
        rh_protocol::message::SyncEntity::Proxy(p) => {
            let mut cur = sess.proxies.lock().await;
            let changed = cur.get(&p.id).map(|o| *o != *p).unwrap_or(true);
            if changed {
                if let Some(old) = cur.remove(&p.id) {
                    let id = p.id;
                    cur.insert(id, old); // 供 stop 读取
                    stop_proxy(sess, p.id).await;
                    cur.remove(&p.id);
                }
                let mut p = p;
                p.normalize_kind_configs();
                cur.insert(p.id, (*p).clone());
                start_health(sess, p.id, &p).await;
            }
        }
        rh_protocol::message::SyncEntity::Visitor(v) => {
            let mut cur = sess.visitors.lock().await;
            let changed = cur.get(&v.id).map(|o| *o != v).unwrap_or(true);
            if changed {
                stop_visitor(sess, v.id).await;
                cur.insert(v.id, v.clone());
                crate::visitor_run::start(sess.clone(), v.id, v).await;
            }
        }
    }
}

async fn remove_entity(sess: &Arc<ClientSession>, entity_type: &str, id: u64) {
    if entity_type == "proxy" {
        sess.proxies.lock().await.remove(&id);
        stop_proxy(sess, id).await;
    } else {
        sess.visitors.lock().await.remove(&id);
        stop_visitor(sess, id).await;
    }
}

async fn stop_proxy(sess: &Arc<ClientSession>, id: u64) {
    if let Some(c) = sess.health_cancels.lock().await.remove(&id) {
        c.cancel();
    }
}

async fn stop_visitor(sess: &Arc<ClientSession>, id: u64) {
    if let Some(c) = sess.visitor_cancels.lock().await.remove(&id) {
        c.cancel();
    }
    sess.sudp_deliver.lock().await.remove(&id);
}

async fn start_health(sess: &Arc<ClientSession>, id: u64, spec: &ProxySpec) {
    let Some(hc) = spec.health_check.clone() else {
        return;
    };
    if sess.health_cancels.lock().await.contains_key(&id) {
        return;
    }
    let c = CancellationToken::new();
    sess.health_cancels.lock().await.insert(id, c.clone());
    let s2 = sess.clone();
    let spec = spec.clone();
    tokio::spawn(async move {
        crate::health::run(s2, id, spec, hc, c).await;
    });
}

// ---------------- 数据通道 ----------------

async fn open_data_channel(sess: Arc<ClientSession>, m: CreateDataChannel) -> anyhow::Result<()> {
    let mut stream = sess.mux.open_stream().await?;
    let mut buf = bytes::BytesMut::with_capacity(512);
    let hello = DataChannelHello {
        session_id: sess.session_id.clone(),
        request_id: m.request_id,
    };
    frame::write_frame(
        &mut stream,
        FrameType::DATA_CHANNEL_HELLO,
        &serde_json::to_vec(&hello)?,
    )
    .await
    .map_err(|e| anyhow::anyhow!("hello write: {e}"))?;
    match m.entity {
        ChannelEntity::Proxy { proxy_id } => {
            let spec = sess.proxies.lock().await.get(&proxy_id).cloned();
            let Some(spec) = spec else {
                anyhow::bail!("unknown proxy {proxy_id}");
            };
            if spec.kind == ProxyKind::Udp || spec.kind == ProxyKind::Sudp {
                // UDP 语义:无指派帧,直接进入 UDP 帧模式(代理侧)
                // hello 之后可能紧跟已抵达的 UDP 帧:预置进帧缓冲
                crate::udpchan::run_proxy_side(stream, spec, buf.to_vec()).await;
                return Ok(());
            }
            // TCP 语义:等待 START_FORWARD(可能延迟到用户连接);
            // 帧后可能紧随用户数据(read_frame 缓冲),必须前传
            let (ty, payload) = frame::read_frame(&mut stream, &mut buf).await?;
            let leftover: Vec<u8> = buf.to_vec();
            if ty != FrameType::START_FORWARD {
                anyhow::bail!("expected START_FORWARD, got {ty:?}");
            }
            let fwd: StartForward = serde_json::from_slice(&payload)?;
            crate::forward::handle_start_forward(sess.clone(), stream, &spec, fwd, leftover)
                .await?;
        }
        ChannelEntity::Visitor {
            visitor_id,
            user_conn_id,
        } => {
            let vspec = sess.visitors.lock().await.get(&visitor_id).cloned();
            let Some(vspec) = vspec else {
                anyhow::bail!("unknown visitor {visitor_id}");
            };
            if user_conn_id == 0 {
                // sudp:无指派帧;交给 visitor 的 UDP 交付槽(预置已到帧)
                let tx = sess.sudp_deliver.lock().await.get(&visitor_id).cloned();
                if let Some(tx) = tx {
                    let _ = tx.send(PrependedIo::wrap(stream, buf.to_vec())).await;
                }
                return Ok(());
            }
            let (ty, payload) = frame::read_frame(&mut stream, &mut buf).await?;
            let leftover: Vec<u8> = buf.to_vec();
            if ty != FrameType::START_VISITOR_RELAY {
                anyhow::bail!("expected START_VISITOR_RELAY, got {ty:?}");
            }
            let relay: StartVisitorRelay = serde_json::from_slice(&payload)?;
            crate::visitor_run::deliver_relay(sess.clone(), relay, stream, vspec, leftover).await;
        }
    }
    Ok(())
}
