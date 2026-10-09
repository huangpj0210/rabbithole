//! AppState:全局运行态(会话注册表、代理运行时、端口分配、组、vhost 路由)
//! 与每客户端串行域。

use crate::config::ServerConfig;
use crate::db::Db;
use crate::events::EventBus;
use crate::session::{Session, SessionRegistry};
use rh_core::net::{bind_conflicts, PortAllowList, PortsRange};
use rh_protocol::{
    message::{ChannelEntity, CreateDataChannel, ServerMessage, SyncEntity},
    ProxySpec,
};
use rh_transport::BoxedIo;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use std::time::Instant;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

/// 中继配对表:((会话 id, request_id) → 通道交付 oneshot)。
pub type RelayPairMap = HashMap<(String, u64), tokio::sync::oneshot::Sender<BoxedIo>>;

pub struct AppState {
    pub cfg: Arc<ServerConfig>,
    pub db: Db,
    pub events: EventBus,
    pub challenge: Arc<crate::control::ChallengeTable>,
    pub sessions: Arc<SessionRegistry>,
    pub proxies: Arc<ProxyManager>,
    pub client_locks: Arc<dashmap::DashMap<String, Arc<Mutex<()>>>>,
    pub metrics: Arc<Metrics>,
    pub shutdown: CancellationToken,
    /// 中继配对表。键含会话 id:request_id 仅在会话内单调,两个会话各自的 rid=1 会撞号。
    pub relay_pairs: Arc<Mutex<RelayPairMap>>,
    /// 活跃 SUDP 常驻中继(visitor_id 集合)。
    pub udp_relays: Arc<Mutex<std::collections::HashSet<u64>>>,
    /// XTCP 打洞协调器。
    pub nathole: Arc<crate::nathole::NatCoordinator>,
    /// 登录限速(每分钟 5 次/用户)。
    pub login_limiter: Arc<governor::DefaultKeyedRateLimiter<String>>,
    /// JWT 密钥(启动时定;空则启动生成随机值)。
    pub web_jwt_secret: Arc<Mutex<Option<String>>>,
}

impl Clone for AppState {
    fn clone(&self) -> Self {
        Self {
            cfg: self.cfg.clone(),
            db: self.db.clone(),
            events: self.events.clone(),
            challenge: self.challenge.clone(),
            sessions: self.sessions.clone(),
            proxies: self.proxies.clone(),
            client_locks: self.client_locks.clone(),
            metrics: self.metrics.clone(),
            shutdown: self.shutdown.clone(),
            relay_pairs: self.relay_pairs.clone(),
            udp_relays: self.udp_relays.clone(),
            nathole: self.nathole.clone(),
            login_limiter: self.login_limiter.clone(),
            web_jwt_secret: self.web_jwt_secret.clone(),
        }
    }
}

impl AppState {
    pub fn new(cfg: ServerConfig, db: Db) -> Self {
        let events = EventBus::new(db.clone());
        let metrics = Arc::new(Metrics::new());
        let cfg = Arc::new(cfg);
        let proxies = Arc::new(ProxyManager::new(cfg.clone()));
        let jwt_secret = cfg
            .web
            .jwt_secret
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        Self {
            challenge: Arc::new(crate::control::ChallengeTable::new()),
            cfg,
            db,
            events,
            sessions: Arc::new(SessionRegistry::new()),
            proxies,
            client_locks: Arc::new(dashmap::DashMap::new()),
            metrics,
            shutdown: CancellationToken::new(),
            relay_pairs: Arc::new(Mutex::new(HashMap::new())),
            udp_relays: Arc::new(Mutex::new(std::collections::HashSet::new())),
            nathole: Arc::new(crate::nathole::NatCoordinator::new()),
            login_limiter: Arc::new(governor::RateLimiter::keyed(governor::Quota::per_minute(
                std::num::NonZeroU32::new(5).unwrap(),
            ))),
            web_jwt_secret: Arc::new(Mutex::new(Some(jwt_secret))),
        }
    }

    /// 每客户端串行域锁(创建/更新/启停/恢复共用)。
    pub fn serial_lock(&self, client_id: &str) -> Arc<Mutex<()>> {
        self.client_locks
            .entry(client_id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

// ---------------- 指标 ----------------

pub struct Metrics {
    pub registry: prometheus::Registry,
    pub connections: prometheus::IntGaugeVec,
    pub client_online: prometheus::IntGauge,
    /// 汇总(控制台 overview)。
    pub totals: std::sync::Mutex<Totals>,
}

#[derive(Default, Clone, Copy)]
pub struct Totals {
    pub conns_current: i64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}

impl Metrics {
    pub fn new() -> Self {
        use prometheus::{self, IntGauge, IntGaugeVec, Opts, Registry};
        let registry = Registry::new();
        let connections = IntGaugeVec::new(
            Opts::new("rabbithole_connections", "active forwarded connections"),
            &["proxy", "kind"],
        )
        .unwrap();
        let client_online = IntGauge::new("rabbithole_clients_online", "online clients").unwrap();
        let _ = registry.register(Box::new(connections.clone()));
        let _ = registry.register(Box::new(client_online.clone()));
        Self {
            registry,
            connections,
            client_online,
            totals: std::sync::Mutex::new(Totals::default()),
        }
    }
}

// ---------------- 代理运行时 ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyState {
    Disabled,
    BindError,
    Offline,
    Pending,
    Running,
    ApplyError,
    Unhealthy,
}

impl ProxyState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ProxyState::Disabled => "disabled",
            ProxyState::BindError => "bind_error",
            ProxyState::Offline => "offline",
            ProxyState::Pending => "pending",
            ProxyState::Running => "running",
            ProxyState::ApplyError => "apply_error",
            ProxyState::Unhealthy => "unhealthy",
        }
    }
}

/// TCP 数据通道池(每代理)。
pub struct TcpPool {
    /// 就绪通道(已完成 DATA_CHANNEL_HELLO,等待 START_FORWARD)。
    /// 元组:(建立该通道的会话 epoch, request_id, io)
    pub ready: Mutex<Vec<(u64, u64, BoxedIo)>>,
    /// 等待用户连接的等待者(用户连接先到)。
    pub waiters: Mutex<Vec<tokio::sync::oneshot::Sender<BoxedIo>>>,
    /// 池容量(池中+在途 ≤ pool_size)。
    pub size: usize,
    /// 在途请求数。
    pub inflight: AtomicU64,
    /// 池当前归属会话的 epoch(全局单调递增;0 = 尚未认领)。
    /// 会话重连/替换后旧条目全部失效,由认领方驱逐。
    pub epoch: AtomicU64,
}

impl TcpPool {
    pub fn new(size: usize) -> Self {
        Self {
            ready: Mutex::new(Vec::new()),
            waiters: Mutex::new(Vec::new()),
            size,
            inflight: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
        }
    }
}

pub struct ProxyRuntime {
    pub id: u64,
    pub client_id: String,
    pub spec: RwLock<ProxySpec>,
    pub state: RwLock<ProxyState>,
    pub state_reason: RwLock<String>,
    pub confirmed_revision: AtomicU64,
    pub latest_revision: AtomicU64,
    pub healthy: AtomicBool,
    /// 独享端口类规则:listener 绑定成功标记(running 的必要条件,§4.1)。
    pub bound: AtomicBool,
    pub rule_cancel: CancellationToken,
    /// tcp/tcpmux(独享或组内成员):Listener 由本运行时或 Group 持有。
    pub tcp_listener: Mutex<Option<TcpListener>>,
    pub udp_socket: Mutex<Option<Arc<UdpSocket>>>,
    pub pool: Mutex<Option<Arc<TcpPool>>>,
    /// UDP pump 通道投递。
    pub udp_new_channel_tx: Mutex<Option<tokio::sync::mpsc::Sender<BoxedIo>>>,
}

impl ProxyRuntime {
    pub fn new(id: u64, client_id: String, spec: ProxySpec) -> Self {
        Self {
            id,
            client_id,
            spec: RwLock::new(spec),
            state: RwLock::new(ProxyState::Offline),
            state_reason: RwLock::new(String::new()),
            confirmed_revision: AtomicU64::new(0),
            latest_revision: AtomicU64::new(0),
            healthy: AtomicBool::new(true),
            bound: AtomicBool::new(false),
            rule_cancel: CancellationToken::new(),
            tcp_listener: Mutex::new(None),
            udp_socket: Mutex::new(None),
            pool: Mutex::new(None),
            udp_new_channel_tx: Mutex::new(None),
        }
    }

    pub async fn set_state(&self, s: ProxyState, reason: &str) {
        let mut st = self.state.write().await;
        let mut r = self.state_reason.write().await;
        tracing::debug!(proxy = self.id, state = s.as_str(), reason, "proxy state");
        *st = s;
        *r = reason.to_string();
    }

    pub async fn snapshot_state(&self) -> (ProxyState, String) {
        (
            *self.state.read().await,
            self.state_reason.read().await.clone(),
        )
    }
}

/// 组负载均衡:同 (kind, bind_ip, bind_port, group) 的成员共享监听。
pub struct GroupRuntime {
    pub members: RwLock<Vec<u64>>,
    pub rr: AtomicU64,
}

// ---------------- ProxyManager ----------------

pub struct ProxyManager {
    pub cfg: Arc<ServerConfig>,
    pub runtimes: dashmap::DashMap<u64, Arc<ProxyRuntime>>,
    /// 组运行时
    pub groups: dashmap::DashMap<String, Arc<GroupRuntime>>,
    /// vhost HTTP 路由表:domain/host → proxy_id
    pub vhost_http: RwLock<Vec<VhostRoute>>,
    pub vhost_https: RwLock<Vec<VhostRoute>>,
    pub vhost_tcpmux: RwLock<Vec<VhostRoute>>,
}

#[derive(Clone)]
pub struct VhostRoute {
    pub host: String,
    pub locations: Vec<String>,
    pub http_user: String,
    pub http_password: String,
    pub host_rewrite: String,
    pub request_headers: rh_protocol::HeaderOp,
    pub response_headers: rh_protocol::HeaderOp,
    pub proxy_id: u64,
}

impl ProxyManager {
    pub fn new(cfg: Arc<ServerConfig>) -> Self {
        Self {
            cfg,
            runtimes: dashmap::DashMap::new(),
            groups: dashmap::DashMap::new(),
            vhost_http: RwLock::new(Vec::new()),
            vhost_https: RwLock::new(Vec::new()),
            vhost_tcpmux: RwLock::new(Vec::new()),
        }
    }

    pub fn get(&self, id: u64) -> Option<Arc<ProxyRuntime>> {
        self.runtimes.get(&id).map(|e| e.value().clone())
    }

    /// 端口占用冲突判定(运行中的独享监听 + 组监听 + DB 其他启用行)。
    pub async fn port_in_use(
        &self,
        proto: &str,
        ip: IpAddr,
        port: u16,
        except_proxy: Option<u64>,
    ) -> bool {
        for e in self.runtimes.iter() {
            let rt = e.value();
            if Some(rt.id) == except_proxy {
                continue;
            }
            let spec = rt.spec.read().await;
            if spec.kind.uses_bind_port()
                && spec.enabled
                && bind_conflicts(
                    proto,
                    ip,
                    port,
                    proto,
                    rh_core::net::parse_bind_ip(&spec.bind_ip)
                        .unwrap_or(IpAddr::from([0, 0, 0, 0])),
                    spec.bind_port,
                )
            {
                return true;
            }
        }
        false
    }

    /// 白名单校验。
    pub fn port_allowed(&self, port: u16) -> bool {
        let allow = PortAllowList {
            ranges: self
                .cfg
                .allow_ports()
                .into_iter()
                .collect::<Vec<PortsRange>>(),
        };
        allow.allows(port)
    }

    /// 从自动分配范围挑选空闲端口(白名单 + 运行时冲突 + OS 探测)。
    pub async fn allocate_port(&self, proto: &str, bind_ip: IpAddr) -> Option<u16> {
        let ranges: Vec<PortsRange> = self.cfg.auto_port_ranges();
        let allow = PortAllowList {
            ranges: self.cfg.allow_ports(),
        };
        for r in ranges {
            for p in r.start..=r.end {
                if !allow.allows(p) {
                    continue;
                }
                if self.port_in_use(proto, bind_ip, p, None).await {
                    continue;
                }
                let addr = SocketAddr::new(bind_ip, p);
                let probe = if proto == "udp" {
                    std::net::UdpSocket::bind(addr).is_ok()
                } else {
                    std::net::TcpListener::bind(addr).is_ok()
                };
                if probe {
                    return Some(p);
                }
            }
        }
        None
    }

    /// 组键。
    pub fn group_key(spec: &ProxySpec) -> Option<String> {
        let group = spec.group.as_deref().unwrap_or("");
        if group.is_empty() || !spec.kind.uses_bind_port() {
            return None;
        }
        Some(format!(
            "{}|{}|{}|{}",
            spec.kind.as_str(),
            spec.bind_ip,
            spec.bind_port,
            group
        ))
    }

    /// vhost 路由重建(简单起见:全量刷新)。
    pub async fn rebuild_vhost_routes(&self) {
        let mut http = Vec::new();
        let mut https = Vec::new();
        let mut tcpmux = Vec::new();
        let subdomain_host = self.cfg.vhost.subdomain_host.clone();
        for e in self.runtimes.iter() {
            let rt = e.value();
            let spec_guard = rt.spec.read().await;
            let spec = &*spec_guard;
            if !spec.enabled {
                continue;
            }
            match spec.kind {
                rh_protocol::ProxyKind::Http => {
                    let Some(c) = &spec.http else { continue };
                    for d in &c.custom_domains {
                        http.push(route_from(spec, rt, d.clone(), c));
                    }
                    if !c.subdomain.is_empty() && !subdomain_host.is_empty() {
                        http.push(route_from(
                            spec,
                            rt,
                            format!("{}.{}", c.subdomain, subdomain_host),
                            c,
                        ));
                    }
                }
                rh_protocol::ProxyKind::Https => {
                    let Some(c) = &spec.https else { continue };
                    for d in &c.custom_domains {
                        https.push(VhostRoute {
                            host: d.clone(),
                            locations: vec![],
                            http_user: String::new(),
                            http_password: String::new(),
                            host_rewrite: String::new(),
                            request_headers: Default::default(),
                            response_headers: Default::default(),
                            proxy_id: rt.id,
                        });
                    }
                    if !c.subdomain.is_empty() && !subdomain_host.is_empty() {
                        https.push(VhostRoute {
                            host: format!("{}.{}", c.subdomain, subdomain_host),
                            locations: vec![],
                            http_user: String::new(),
                            http_password: String::new(),
                            host_rewrite: String::new(),
                            request_headers: Default::default(),
                            response_headers: Default::default(),
                            proxy_id: rt.id,
                        });
                    }
                }
                rh_protocol::ProxyKind::TcpMux => {
                    let Some(c) = &spec.tcpmux else { continue };
                    for d in &c.custom_domains {
                        tcpmux.push(VhostRoute {
                            host: d.clone(),
                            locations: vec![],
                            http_user: c.http_user.clone(),
                            http_password: c.http_password.clone(),
                            host_rewrite: String::new(),
                            request_headers: Default::default(),
                            response_headers: Default::default(),
                            proxy_id: rt.id,
                        });
                    }
                }
                _ => {}
            }
        }
        *self.vhost_http.write().await = http;
        *self.vhost_https.write().await = https;
        *self.vhost_tcpmux.write().await = tcpmux;
    }
}

fn route_from(
    _spec: &ProxySpec,
    rt: &ProxyRuntime,
    host: String,
    c: &rh_protocol::HttpProxyConfig,
) -> VhostRoute {
    VhostRoute {
        host,
        locations: c.locations.clone(),
        http_user: c.http_user.clone(),
        http_password: c.http_password.clone(),
        host_rewrite: c.host_rewrite.clone(),
        request_headers: c.request_headers.clone(),
        response_headers: c.response_headers.clone(),
        proxy_id: rt.id,
    }
}

// ---------------- 数据通道请求签发 ----------------
impl AppState {
    /// 向客户端签发一个数据通道请求(服务端指令发起)。
    pub async fn issue_create_channel(
        &self,
        sess: &Arc<Session>,
        entity: ChannelEntity,
        purpose: crate::session::RequestPurpose,
    ) -> Option<u64> {
        let rid = sess.alloc_request_id();
        sess.add_pending_request(
            rid,
            crate::session::PendingRequest {
                expires: Instant::now() + std::time::Duration::from_secs(10),
                purpose,
            },
        )
        .await;
        let ok = sess
            .send(ServerMessage::CreateDataChannel(CreateDataChannel {
                request_id: rid,
                entity,
            }))
            .await;
        if ok {
            Some(rid)
        } else {
            sess.take_pending_request(rid).await;
            None
        }
    }

    /// 中继双侧通道签发:配对表登记必须先于 CREATE 帧可达。
    /// localhost 下客户端可能在签发任务恢复执行前就完成开流+HELLO 回环,
    /// 若配对表后登记,deliver_pair 会找不到槽位而静默丢流。
    #[allow(clippy::type_complexity)]
    pub async fn issue_relay_pair(
        &self,
        visitor_sess: &Arc<Session>,
        proxy_sess: &Arc<Session>,
        visitor_entity: ChannelEntity,
        visitor_purpose: crate::session::RequestPurpose,
        proxy_entity: ChannelEntity,
        proxy_purpose: crate::session::RequestPurpose,
    ) -> Option<(
        u64,
        u64,
        tokio::sync::oneshot::Receiver<BoxedIo>,
        tokio::sync::oneshot::Receiver<BoxedIo>,
    )> {
        let v_rid = visitor_sess.alloc_request_id();
        let p_rid = proxy_sess.alloc_request_id();
        let (vt, vr) = tokio::sync::oneshot::channel::<BoxedIo>();
        let (pt, pr) = tokio::sync::oneshot::channel::<BoxedIo>();
        {
            let mut pairs = self.relay_pairs.lock().await;
            pairs.insert((visitor_sess.session_id.clone(), v_rid), vt);
            pairs.insert((proxy_sess.session_id.clone(), p_rid), pt);
        }
        let ttl = Instant::now() + std::time::Duration::from_secs(10);
        visitor_sess
            .add_pending_request(
                v_rid,
                crate::session::PendingRequest {
                    expires: ttl,
                    purpose: visitor_purpose,
                },
            )
            .await;
        proxy_sess
            .add_pending_request(
                p_rid,
                crate::session::PendingRequest {
                    expires: ttl,
                    purpose: proxy_purpose,
                },
            )
            .await;
        let ok_v = visitor_sess
            .send(ServerMessage::CreateDataChannel(CreateDataChannel {
                request_id: v_rid,
                entity: visitor_entity,
            }))
            .await;
        let ok_p = proxy_sess
            .send(ServerMessage::CreateDataChannel(CreateDataChannel {
                request_id: p_rid,
                entity: proxy_entity,
            }))
            .await;
        if ok_v && ok_p {
            return Some((v_rid, p_rid, vr, pr));
        }
        tracing::warn!(
            v_rid,
            p_rid,
            sent_v = ok_v,
            sent_p = ok_p,
            "relay pair create send failed"
        );
        visitor_sess.take_pending_request(v_rid).await;
        proxy_sess.take_pending_request(p_rid).await;
        let mut pairs = self.relay_pairs.lock().await;
        pairs.remove(&(visitor_sess.session_id.clone(), v_rid));
        pairs.remove(&(proxy_sess.session_id.clone(), p_rid));
        None
    }

    /// 组装某客户端的全量 SYNC 实体集。
    pub async fn build_sync_entities(&self, client_id: &str) -> Vec<SyncEntity> {
        let mut out = Vec::new();
        if let Ok(proxies) = self.db.list_proxies(Some(client_id)).await {
            for (_id, _cid, spec) in proxies {
                if spec.enabled {
                    out.push(SyncEntity::Proxy(Box::new(spec)));
                }
            }
        }
        if let Ok(visitors) = self.db.list_visitors(Some(client_id)).await {
            for (_id, _cid, _target, spec) in visitors {
                if spec.enabled {
                    out.push(SyncEntity::Visitor(spec));
                }
            }
        }
        out
    }
}
