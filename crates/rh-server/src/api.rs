//! 管理 REST API(axum):vben 契约 + 客户端/规则/visitor/统计/事件/指标。

use crate::db::normalize_spec;
use crate::state::{AppState, ProxyState};
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post, put};
use axum::{Json, Router};
use rh_protocol::{ProxyKind, ProxySpec, VisitorSpec};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

pub fn router(state: Arc<AppState>) -> Router {
    let auth = Router::new()
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(change_password))
        .route("/api/user/info", get(user_info))
        .route("/api/auth/codes", get(auth_codes))
        .route("/api/menu/all", get(menu_all));
    let clients = Router::new()
        .route("/api/clients", get(list_clients).post(create_client))
        .route(
            "/api/clients/{id}",
            put(update_client).delete(delete_client),
        )
        .route("/api/clients/{id}/rotate-secret", post(rotate_secret));
    let services = Router::new()
        .route("/api/services", get(list_services).post(create_service))
        .route(
            "/api/services/{id}",
            put(update_service).delete(delete_service),
        )
        .route("/api/services/{id}/enable", post(enable_service))
        .route("/api/services/{id}/disable", post(disable_service))
        .route("/api/services/batch", post(batch_services));
    let visitors = Router::new()
        .route("/api/visitors", get(list_visitors).post(create_visitor))
        .route(
            "/api/visitors/{id}",
            put(update_visitor).delete(delete_visitor),
        )
        .route("/api/visitors/{id}/enable", post(enable_visitor))
        .route("/api/visitors/{id}/disable", post(disable_visitor));
    let misc = Router::new()
        .route("/api/stats/overview", get(stats_overview))
        .route("/api/events", get(list_events));
    let mut r = auth
        .merge(clients)
        .merge(services)
        .merge(visitors)
        .merge(misc);
    // Prometheus 指标端点:仅在显式开启时注册,并要求与管理 API 相同的 Bearer 认证
    if state.cfg.metrics.enable_prometheus {
        r = r.route("/metrics", get(prometheus_metrics));
    }
    r.with_state(state)
}

async fn menu_all(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    ok(serde_json::json!([]))
}

// ---------------- envelope ----------------

pub fn ok<T: Serialize>(data: T) -> Response {
    Json(json!({"code": 0, "data": data, "message": "ok"})).into_response()
}

pub fn err(code: i32, message: &str) -> Response {
    Json(json!({"code": code, "message": message, "data": null})).into_response()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"code": 401, "message": "unauthorized", "data": null})),
    )
        .into_response()
}

// ---------------- auth ----------------

#[derive(Deserialize)]
struct LoginReq {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<AppState>>,
    ConnectInfo(peer): ConnectInfo<crate::WebAddr>,
    Json(req): Json<LoginReq>,
) -> Response {
    // 登录限速:每 IP+用户名 5 次/分钟(governor)。
    // 用户名截断防任意长度 key 撑内存。
    let uname: String = req.username.chars().take(64).collect();
    if state
        .login_limiter
        .check_key(&format!("{peer}|{uname}"))
        .is_err()
    {
        state
            .events
            .emit("warn", "auth", &format!("登录尝试受限:{uname}"))
            .await;
        return err(429, "尝试过于频繁,请稍后再试");
    }
    let user = match state.db.get_user(&req.username).await {
        Ok(Some(u)) => u,
        _ => {
            // 对不存在的用户也执行一次等价 argon2 校验,消除时序侧信道枚举
            const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$OXRoZXJlYXJlZnJwaHJvbGl0Z$5ENkQGI9XCo8uUU6eW/y6vLzRnF8YD2NO6wanjaApfE";
            let _ = verify_password(&req.password, DUMMY_HASH);
            state
                .events
                .emit("warn", "auth", &format!("登录失败(用户不存在):{uname}"))
                .await;
            return err(1001, "用户名或密码错误");
        }
    };
    if !verify_password(&req.password, &user.1) {
        state
            .events
            .emit(
                "warn",
                "auth",
                &format!("登录失败(密码错误):{}", req.username),
            )
            .await;
        return err(1001, "用户名或密码错误");
    }
    let ttl = chrono_hours(state.cfg.web.jwt_ttl_hours);
    let token = match mint_jwt(&state, &req.username, ttl).await {
        Ok(t) => t,
        Err(e) => return err(500, &format!("内部错误(jwt):{e}")),
    };
    state
        .events
        .emit("info", "auth", &format!("登录成功:{}", req.username))
        .await;
    ok(json!({ "accessToken": token }))
}

#[derive(Deserialize)]
struct ChangePasswordReq {
    old_password: String,
    new_password: String,
}

/// 修改自身密码:验旧密 → Argon2 落库 → 轮换 JWT 密钥(所有已发令牌立即失效,
/// 含当前会话,前端需回到登录页)。新密码最短 8 位。
async fn change_password(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<ChangePasswordReq>,
) -> Response {
    let user = match require_auth(&state, &headers).await {
        Ok(u) => u,
        Err(resp) => return *resp,
    };
    if req.new_password.len() < 8 {
        return err(1002, "新密码至少需要 8 位");
    }
    let row = match state.db.get_user(&user).await {
        Ok(Some(r)) => r,
        _ => return err(1001, "用户不存在"),
    };
    if !verify_password(&req.old_password, &row.1) {
        state
            .events
            .emit(
                "warn",
                "auth",
                &format!("修改密码被拒(旧密码错误):{}", user),
            )
            .await;
        return err(1001, "旧密码错误");
    }
    let hash = match hash_password(&req.new_password) {
        Ok(h) => h,
        Err(e) => return err(500, &format!("内部错误(hash):{e}")),
    };
    match state.db.update_user_password(&user, &hash).await {
        Ok(true) => {}
        _ => return err(500, "更新失败"),
    }
    // 轮换会话密钥:旧令牌(含本请求所用)全部失效
    *state.web_jwt_secret.lock().await = Some(uuid::Uuid::new_v4().simple().to_string());
    state
        .events
        .emit("info", "auth", &format!("密码已修改:{}", user))
        .await;
    ok(json!({ "relogin": true }))
}

fn chrono_hours(h: u64) -> u64 {
    h
}

fn verify_password(plain: &str, hash: &str) -> bool {
    use argon2::PasswordVerifier;
    argon2::password_hash::PasswordHash::new(hash)
        .map(|parsed| {
            argon2::Argon2::default()
                .verify_password(plain.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn hash_password(plain: &str) -> anyhow::Result<String> {
    use argon2::password_hash::{PasswordHasher, SaltString};
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    Ok(argon2::Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("hash: {e:?}"))?
        .to_string())
}

async fn mint_jwt(state: &Arc<AppState>, username: &str, ttl_hours: u64) -> anyhow::Result<String> {
    let secret = state.jwt_secret().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let claims = json!({
        "sub": username,
        "iat": now,
        "exp": now + (ttl_hours as i64) * 3600,
    });
    Ok(jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )?)
}

impl AppState {
    pub async fn jwt_secret(&self) -> String {
        self.web_jwt_secret.lock().await.clone().unwrap_or_default()
    }
}

/// JWT 校验中间件:返回 username 或 401 response(装箱避免超大 Err 变体)。
async fn require_auth(state: &Arc<AppState>, headers: &HeaderMap) -> Result<String, Box<Response>> {
    let bearer = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if bearer.is_empty() {
        return Err(Box::new(unauthorized()));
    }
    let secret = state.jwt_secret().await;
    let data = jsonwebtoken::decode::<serde_json::Value>(
        bearer,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    );
    match data {
        Ok(d) => Ok(d
            .claims
            .get("sub")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()),
        Err(_) => Err(Box::new(unauthorized())),
    }
}

async fn logout(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    match require_auth(&state, &headers).await {
        Ok(user) => {
            state
                .events
                .emit("info", "auth", &format!("已登出:{user}"))
                .await;
            StatusCode::NO_CONTENT.into_response()
        }
        Err(r) => *r,
    }
}

async fn user_info(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let user = match require_auth(&state, &headers).await {
        Ok(u) => u,
        Err(r) => return *r,
    };
    ok(json!({
        "userId": user,
        "username": user,
        "realName": user,
        "avatar": "",
        "desc": "rabbithole administrator",
        "homePath": "/rabbithole/overview",
        "roles": ["admin"],
    }))
}

async fn auth_codes(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    ok(json!([]))
}

// ---------------- clients ----------------

async fn list_clients(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let rows = match state.db.list_clients().await {
        Ok(r) => r,
        Err(e) => return err(500, &format!("{e}")),
    };
    let out: Vec<_> = rows
        .into_iter()
        .map(|c| {
            let sess = state.sessions.get(&c.id);
            json!({
                "id": c.id,
                "name": c.name,
                "enabled": c.enabled,
                "remark": c.remark,
                "online": sess.is_some(),
                "online_since": sess.as_ref().map(|s| s.started_at.elapsed().as_secs()),
                "reported_revision": sess.as_ref().map(|s| s.applied()),
                "expected_revision": c.config_revision,
                "client_version": c.client_version,
                "hostname": c.hostname,
                "os": c.os,
                "last_online_at": c.last_online_at,
                "created_at": c.created_at,
            })
        })
        .collect();
    ok(out)
}

#[derive(Deserialize)]
struct CreateClientReq {
    name: String,
    #[serde(default)]
    remark: String,
}

async fn create_client(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateClientReq>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 64 {
        return err(1002, "名称长度须为 1-64 字符");
    }
    let id = uuid::Uuid::new_v4().to_string();
    let secret = crate::control::hmac_hex(
        uuid::Uuid::new_v4().as_bytes(),
        &uuid::Uuid::new_v4().to_string(),
    );
    // 64 hex
    let secret = format!("{secret}{secret}");
    let secret = secret[..64].to_string();
    if let Err(e) = state
        .db
        .create_client(&id, name, &secret, &req.remark)
        .await
    {
        return err(1003, &format!("创建失败:{e}"));
    }
    state
        .events
        .emit("info", "client", &format!("客户端已创建:{name}"))
        .await;
    ok(json!({
        "id": id,
        "name": name,
        "remark": req.remark,
        "secret": secret,   // 仅本次返回
    }))
}

#[derive(Deserialize)]
struct UpdateClientReq {
    enabled: Option<bool>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    remark: Option<String>,
}

async fn update_client(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<UpdateClientReq>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    if let Some(name) = &req.name {
        let name = name.trim();
        if name.is_empty() || name.len() > 64 {
            return err(1002, "名称长度须为 1-64 字符");
        }
    }
    let lock = state.serial_lock(&id);
    let _g = lock.lock().await;
    let Some(_c) = state.db.get_client(&id).await.ok().flatten() else {
        return err(1004, "客户端不存在");
    };
    if req.name.is_some() || req.remark.is_some() {
        let name = req.name.as_deref().map(str::trim);
        if let Err(e) = state
            .db
            .update_client_profile(&id, name, req.remark.as_deref().map(|r| r.trim()))
            .await
        {
            return err(500, &format!("{e}"));
        }
        state
            .events
            .emit("info", "client", &format!("客户端资料已更新:{}", _c.name))
            .await;
    }
    if let Some(enabled) = req.enabled {
        if let Err(e) = state.db.set_client_enabled(&id, enabled).await {
            return err(500, &format!("{e}"));
        }
        if enabled {
            crate::runtime::notify_client_recovery(&state, &id).await;
        } else {
            // 禁用:踢会话 + 全部规则 rule_cancel + REMOVE 入队
            if let Some(sess) = state.sessions.get(&id) {
                sess.cancel.cancel();
            }
            let proxies = state.db.list_proxies(Some(&id)).await.unwrap_or_default();
            let visitors = state.db.list_visitors(Some(&id)).await.unwrap_or_default();
            let rev = state.db.bump_revision(&id).await.unwrap_or(0);
            for (pid, _, _spec) in &proxies {
                crate::runtime::teardown_runtime(&state, *pid as u64).await;
                if let Some(sess) = state.sessions.get(&id) {
                    use rh_protocol::message::{ServerMessage, ServiceRemove};
                    let _ = sess
                        .send(ServerMessage::ServiceRemove(ServiceRemove {
                            revision: rev,
                            entity_type: "proxy".into(),
                            entity_id: *pid as u64,
                        }))
                        .await;
                }
            }
            for (vid, _, _, _) in &visitors {
                if let Some(sess) = state.sessions.get(&id) {
                    use rh_protocol::message::{ServerMessage, ServiceRemove};
                    let _ = sess
                        .send(ServerMessage::ServiceRemove(ServiceRemove {
                            revision: rev,
                            entity_type: "visitor".into(),
                            entity_id: *vid as u64,
                        }))
                        .await;
                }
            }
            state
                .events
                .emit("warn", "client", &format!("客户端已禁用:{}", _c.name))
                .await;
        }
    }
    ok(json!({"updated": true}))
}

async fn rotate_secret(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let lock = state.serial_lock(&id);
    let _g = lock.lock().await;
    let new_secret = crate::control::hmac_hex(
        uuid::Uuid::new_v4().as_bytes(),
        &uuid::Uuid::new_v4().to_string(),
    );
    let new_secret = format!("{new_secret}{new_secret}");
    let new_secret = new_secret[..64].to_string();
    if let Err(e) = state.db.rotate_client_secret(&id, &new_secret).await {
        return err(500, &format!("{e}"));
    }
    // 踢线重认证(listener 不动)
    if let Some(sess) = state.sessions.get(&id) {
        sess.cancel.cancel();
    }
    state
        .events
        .emit("warn", "client", "客户端密钥已轮换")
        .await;
    ok(json!({ "secret": new_secret }))
}

async fn delete_client(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let lock = state.serial_lock(&id);
    let _g = lock.lock().await;
    let proxies = state.db.list_proxies(Some(&id)).await.unwrap_or_default();
    for (pid, _, _) in &proxies {
        crate::runtime::teardown_runtime(&state, *pid as u64).await;
    }
    if let Some(sess) = state.sessions.get(&id) {
        sess.cancel.cancel();
    }
    if let Err(e) = state.db.delete_client(&id).await {
        return err(500, &format!("{e}"));
    }
    state.events.emit("warn", "client", "客户端已删除").await;
    ok(json!({"deleted": true}))
}

// ---------------- services(proxies) ----------------

#[derive(Deserialize)]
struct ListQuery {
    client_id: Option<String>,
}

async fn list_services(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let rows = match state.db.list_proxies(q.client_id.as_deref()).await {
        Ok(r) => r,
        Err(e) => return err(500, &format!("{e}")),
    };
    let mut out = Vec::new();
    for (id, client_id, mut spec) in rows {
        normalize_spec(&mut spec, state.cfg.limits.default_pool_size);
        let (status, reason) = match state.proxies.get(id as u64) {
            Some(rt) => {
                let (st, r) = rt.snapshot_state().await;
                (st.as_str().to_string(), r)
            }
            None => ("disabled".into(), String::new()),
        };
        let pool_ready = match state.proxies.get(id as u64) {
            Some(rt) => match rt.pool.lock().await.as_ref() {
                Some(p) => p.ready.lock().await.len(),
                None => 0,
            },
            None => 0,
        };
        out.push(json!({
            "id": id,
            "client_id": client_id,
            "name": spec.name,
            "kind": spec.kind.as_str(),
            "enabled": spec.enabled,
            "bind_ip": spec.bind_ip,
            "bind_port": spec.bind_port,
            "target": format!("{}:{}", spec.target_host, spec.target_port),
            "target_host": spec.target_host,
            "target_port": spec.target_port,
            "plugin": spec.plugin.as_ref().map(|p| p.kind().as_str().to_string()),
            "group": spec.group,
            "status": status,
            "status_reason": reason,
            "healthy": state.proxies.get(id as u64).map(|rt| rt.healthy.load(std::sync::atomic::Ordering::Relaxed)).unwrap_or(true),
            "pool_ready": pool_ready,
            "spec": spec,
        }));
    }
    ok(out)
}

async fn create_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(mut spec): Json<ProxySpec>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    match create_service_inner(&state, &mut spec).await {
        Ok(v) => ok(v),
        Err((code, msg)) => err(code, &msg),
    }
}

async fn create_service_inner(
    state: &Arc<AppState>,
    spec: &mut ProxySpec,
) -> Result<serde_json::Value, (i32, String)> {
    // 基础校验
    if spec.name.is_empty()
        || !spec
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || spec.name.len() > 64
    {
        return Err((2001, "名称须为 1-64 位字母/数字/下划线/连字符".into()));
    }
    normalize_spec(spec, state.cfg.limits.default_pool_size);
    if spec.client_id.is_empty() {
        return Err((2002, "缺少 client_id".into()));
    }
    let client = state
        .db
        .get_client(&spec.client_id)
        .await
        .map_err(|e| (500, format!("{e}")))?
        .ok_or((2002, "客户端不存在".to_string()))?;
    let client_enabled = client.enabled;
    spec.enabled = spec.enabled && client_enabled;
    let lock = state.serial_lock(&client.id);
    let _g = lock.lock().await;
    // 每客户端数量上限(串行域内计数,防并发创建突破配额)
    if state.cfg.ports.max_services_per_client > 0 {
        let count = state
            .db
            .list_proxies(Some(&client.id))
            .await
            .map_err(|e| (500, format!("{e}")))?
            .len();
        if count >= state.cfg.ports.max_services_per_client {
            return Err((2003, "LIMIT_EXCEEDED:超出配额限制".into()));
        }
    }
    // 资格跳步(§4.4):客户端禁用 → 仅落库为 disabled(不占端口、不推进 revision)
    if !client_enabled {
        let id = state
            .db
            .insert_proxy(&client.id, spec, false, "console")
            .await
            .map_err(|e| (2010, format!("{e}")))?;
        spec.id = id as u64;
        let cur_rev = client.config_revision.max(0) as u64;
        crate::runtime::spawn_runtime(state, &client.id, spec, cur_rev).await;
        return Ok(json!({
            "id": id,
            "bind_port": spec.bind_port,
            "port_auto": false,
            "deferred": true,
        }));
    }
    // 端口处理(tcp/udp)
    let mut port_auto = false;
    if spec.kind.uses_bind_port() {
        let bind_ip = rh_core::net::parse_bind_ip(&spec.bind_ip)
            .ok_or((2004, "invalid bind_ip".to_string()))?;
        if spec.bind_port == 0 {
            port_auto = true;
        } else {
            if !state.proxies.port_allowed(spec.bind_port) {
                return Err((2005, "PORT_DENIED:端口不在允许范围".into()));
            }
            let proto = spec.kind.as_str();
            let gkey = crate::state::ProxyManager::group_key(spec);
            if state
                .proxies
                .port_in_use(proto, bind_ip, spec.bind_port, None, gkey.clone())
                .await
            {
                return Err((2006, "PORT_CONFLICT:端口已被占用".into()));
            }
            // 同组共享监听已在位时无需(也无法)再次试绑
            let group_bound = state.proxies.group_listener_bound(&gkey);
            let trial_ok = group_bound
                || if spec.kind == ProxyKind::Tcp {
                    trial_bind_tcp(bind_ip, spec.bind_port)
                } else {
                    trial_bind_udp(bind_ip, spec.bind_port)
                };
            if !trial_ok {
                return Err((2007, "端口绑定失败:端口已被占用".into()));
            }
        }
    } else if matches!(
        spec.kind,
        ProxyKind::Http | ProxyKind::Https | ProxyKind::TcpMux
    ) {
        // vhost 必须有域名或子域
        let has_domain = match spec.kind {
            ProxyKind::Http => spec
                .http_cfg()
                .map(|c| !c.custom_domains.is_empty() || !c.subdomain.is_empty())
                .unwrap_or(false),
            ProxyKind::Https => spec
                .https_cfg()
                .map(|c| !c.custom_domains.is_empty() || !c.subdomain.is_empty())
                .unwrap_or(false),
            ProxyKind::TcpMux => spec
                .tcpmux_cfg()
                .map(|c| !c.custom_domains.is_empty())
                .unwrap_or(false),
            _ => false,
        };
        if !has_domain {
            return Err((
                2008,
                "vhost proxy requires custom_domains or subdomain".into(),
            ));
        }
    }
    // 分配端口
    if port_auto {
        let bind_ip = rh_core::net::parse_bind_ip(&spec.bind_ip)
            .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));

        let port = state
            .proxies
            .allocate_port(spec.kind.as_str(), bind_ip)
            .await
            .ok_or((2009, "no free port in auto range".to_string()))?;
        spec.bind_port = port;
    }
    // 落库 + revision(单事务,§4.4 步骤 4)
    let (id, rev) = state
        .db
        .insert_proxy_bump(&client.id, spec, port_auto, "console")
        .await
        .map_err(|e| (2010, format!("{e}")))?;
    spec.id = id as u64;
    // 发布运行态
    crate::runtime::spawn_runtime(state, &client.id, spec, rev).await;
    state
        .events
        .emit("info", "service", &format!("转发规则已创建:{}", spec.name))
        .await;
    Ok(json!({
        "id": id,
        "bind_port": spec.bind_port,
        "port_auto": port_auto,
        "revision": rev,
    }))
}

async fn update_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(mut spec): Json<ProxySpec>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    spec.id = id as u64;
    // 名称校验与创建一致(名字是 visitor 目标解析的稳定键)
    if spec.name.is_empty()
        || !spec
            .name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        || spec.name.len() > 64
    {
        return err(2001, "名称须为 1-64 位字母/数字/下划线/连字符");
    }
    normalize_spec(&mut spec, state.cfg.limits.default_pool_size);
    // 实体读取在串行域内进行,避免与并发管理操作竞态
    let Some(client_id) = state
        .db
        .get_proxy(id)
        .await
        .ok()
        .flatten()
        .map(|(_, cid, _, _)| cid)
    else {
        return err(3001, "转发规则不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, client_id, old, port_auto)) = state.db.get_proxy(id).await.ok().flatten() else {
        return err(3001, "转发规则不存在");
    };
    let client = state.db.get_client(&client_id).await.ok().flatten();
    let client_enabled = client.as_ref().map(|c| c.enabled).unwrap_or(false);
    // 资格跳步:规则或客户端禁用 → 仅落库
    if !old.enabled || !client_enabled {
        if let Err(e) = state.db.update_proxy(id, &spec, port_auto).await {
            return err(500, &format!("{e}"));
        }
        return ok(json!({"updated": true, "deferred": true}));
    }
    // 更新路径不允许 bind_port=0(随机端口语义仅创建时支持)
    if spec.kind.uses_bind_port() && spec.bind_port == 0 {
        return err(3006, "bind_port 不能为 0(仅创建时支持自动分配)");
    }
    // 非端口字段变更(§4.3):listener/池不动,原位发布快照 + 入队
    let port_changed =
        spec.kind != old.kind || spec.bind_port != old.bind_port || spec.bind_ip != old.bind_ip;
    if !port_changed {
        if let Some(rt) = state.proxies.get(id as u64) {
            let rev = match state
                .db
                .update_proxy_bump(id, &client_id, &spec, port_auto)
                .await
            {
                Ok(r) => r,
                Err(e) => return err(500, &format!("{e}")),
            };
            crate::runtime::publish_spec_in_place(&state, &rt, &spec, rev).await;
            return ok(json!({"updated": true, "revision": rev}));
        }
    }
    // bind 变更:重叠地址拒绝 + 试绑定(tcp 与 udp 均试;失败不动 DB 与旧运行态)
    if spec.kind.uses_bind_port() {
        let new_ip = rh_core::net::parse_bind_ip(&spec.bind_ip)
            .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
        let old_ip = rh_core::net::parse_bind_ip(&old.bind_ip)
            .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
        if rh_core::net::bind_conflicts(
            spec.kind.as_str(),
            new_ip,
            spec.bind_port,
            old.kind.as_str(),
            old_ip,
            old.bind_port,
        ) && (new_ip != old_ip || spec.bind_port == old.bind_port)
        {
            return err(3002, "OVERLAP_ADDR_UPDATE:绑定地址重叠,不支持直接修改");
        }
        if !state.proxies.port_allowed(spec.bind_port) {
            return err(3003, "PORT_DENIED:端口不在允许范围");
        }
        let gkey = crate::state::ProxyManager::group_key(&spec);
        if state
            .proxies
            .port_in_use(
                spec.kind.as_str(),
                new_ip,
                spec.bind_port,
                Some(id as u64),
                gkey.clone(),
            )
            .await
        {
            return err(3004, "PORT_CONFLICT:端口已被占用");
        }
        let group_bound = state.proxies.group_listener_bound(&gkey);
        let trial_ok = group_bound
            || if spec.kind == ProxyKind::Tcp {
                trial_bind_tcp(new_ip, spec.bind_port)
            } else {
                trial_bind_udp(new_ip, spec.bind_port)
            };
        if !trial_ok {
            return err(3005, "端口绑定失败");
        }
    }
    let rev = match state
        .db
        .update_proxy_bump(id, &client_id, &spec, port_auto)
        .await
    {
        Ok(r) => r,
        Err(e) => return err(500, &format!("{e}")),
    };
    // 端口/类型变更:重建运行态(在途连接由 rule_cancel 级联关闭)
    crate::runtime::teardown_runtime(&state, id as u64).await;
    crate::runtime::spawn_runtime(&state, &client_id, &spec, rev).await;
    ok(json!({"updated": true, "revision": rev}))
}

async fn enable_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let Some(client_id) = state
        .db
        .get_proxy(id)
        .await
        .ok()
        .flatten()
        .map(|(_, cid, _, _)| cid)
    else {
        return err(3001, "转发规则不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, client_id, mut spec, port_auto)) = state.db.get_proxy(id).await.ok().flatten()
    else {
        return err(3001, "转发规则不存在");
    };
    let client_enabled = state
        .db
        .get_client(&client_id)
        .await
        .ok()
        .flatten()
        .map(|c| c.enabled)
        .unwrap_or(false);
    // 客户端禁用:仅置 enabled=1 落库(不推进 revision、不 bind,等客户端恢复)
    if !client_enabled {
        state.db.set_proxy_enabled(id, true).await.ok();
        spec.enabled = true;
        let cur_rev = state
            .db
            .get_client(&client_id)
            .await
            .ok()
            .flatten()
            .map(|c| c.config_revision.max(0) as u64)
            .unwrap_or(0);
        crate::runtime::teardown_runtime(&state, id as u64).await;
        crate::runtime::spawn_runtime(&state, &client_id, &spec, cur_rev).await;
        return ok(json!({"enabled": true, "deferred": true}));
    }
    state.db.set_proxy_enabled(id, true).await.ok();
    spec.enabled = true;
    // 资格校验 + bind + 入队(同创建);失败回滚 enabled
    match create_bind_checked(&state, &client_id, &mut spec, port_auto, id).await {
        Ok(()) => {}
        Err((code, msg)) => {
            state.db.set_proxy_enabled(id, false).await.ok();
            return err(code, &msg);
        }
    }
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    crate::runtime::teardown_runtime(&state, id as u64).await;
    crate::runtime::spawn_runtime(&state, &client_id, &spec, rev).await;
    ok(json!({"enabled": true, "revision": rev}))
}

/// enable 时的绑定资格检查(端口冲突/试绑)。
/// 试绑 TCP:macOS 上通配 bind 可与既有具体地址监听共存(Linux 拒绝);
/// 通配时补一次环回探针,使两平台判定一致。
fn trial_bind_tcp(ip: std::net::IpAddr, port: u16) -> bool {
    if !std::net::TcpListener::bind(std::net::SocketAddr::new(ip, port)).is_ok() {
        return false;
    }
    if ip.is_unspecified() {
        return std::net::TcpListener::bind(std::net::SocketAddr::new(
            std::net::IpAddr::from([127, 0, 0, 1]),
            port,
        ))
        .is_ok();
    }
    true
}

/// 试绑 UDP(同上)。
fn trial_bind_udp(ip: std::net::IpAddr, port: u16) -> bool {
    if !std::net::UdpSocket::bind(std::net::SocketAddr::new(ip, port)).is_ok() {
        return false;
    }
    if ip.is_unspecified() {
        return std::net::UdpSocket::bind(std::net::SocketAddr::new(
            std::net::IpAddr::from([127, 0, 0, 1]),
            port,
        ))
        .is_ok();
    }
    true
}

async fn create_bind_checked(
    state: &Arc<AppState>,
    _client_id: &str,
    spec: &mut ProxySpec,
    _port_auto: bool,
    id: i64,
) -> Result<(), (i32, String)> {
    if !spec.kind.uses_bind_port() {
        return Ok(());
    }
    let bind_ip =
        rh_core::net::parse_bind_ip(&spec.bind_ip).ok_or((2004, "invalid bind_ip".to_string()))?;
    if !state.proxies.port_allowed(spec.bind_port) {
        return Err((2005, "PORT_DENIED:端口不在允许范围".into()));
    }
    let gkey = crate::state::ProxyManager::group_key(spec);
    if state
        .proxies
        .port_in_use(
            spec.kind.as_str(),
            bind_ip,
            spec.bind_port,
            Some(id as u64),
            gkey.clone(),
        )
        .await
    {
        return Err((2006, "PORT_CONFLICT:端口已被占用".into()));
    }
    let group_bound = state.proxies.group_listener_bound(&gkey);
    let trial_ok = group_bound
        || if spec.kind == ProxyKind::Tcp {
            trial_bind_tcp(bind_ip, spec.bind_port)
        } else {
            trial_bind_udp(bind_ip, spec.bind_port)
        };
    if !trial_ok {
        return Err((2007, "端口绑定失败".into()));
    }
    Ok(())
}

async fn disable_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let Some(client_id) = state
        .db
        .get_proxy(id)
        .await
        .ok()
        .flatten()
        .map(|(_, cid, _, _)| cid)
    else {
        return err(3001, "转发规则不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, client_id, spec, _)) = state.db.get_proxy(id).await.ok().flatten() else {
        return err(3001, "转发规则不存在");
    };
    state.db.set_proxy_enabled(id, false).await.ok();
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    crate::runtime::teardown_runtime(&state, id as u64).await;
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceRemove};
        let _ = sess
            .send(ServerMessage::ServiceRemove(ServiceRemove {
                revision: rev,
                entity_type: "proxy".into(),
                entity_id: id as u64,
            }))
            .await;
    }
    state
        .events
        .emit("info", "service", &format!("转发规则已停用:{}", spec.name))
        .await;
    ok(json!({"disabled": true, "revision": rev}))
}

async fn delete_service(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let Some(client_id) = state
        .db
        .get_proxy(id)
        .await
        .ok()
        .flatten()
        .map(|(_, cid, _, _)| cid)
    else {
        return err(3001, "转发规则不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, client_id, spec, _)) = state.db.get_proxy(id).await.ok().flatten() else {
        return err(3001, "转发规则不存在");
    };
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    crate::runtime::teardown_runtime(&state, id as u64).await;
    if let Err(e) = state.db.delete_proxy(id).await {
        return err(500, &format!("{e}"));
    }
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceRemove};
        let _ = sess
            .send(ServerMessage::ServiceRemove(ServiceRemove {
                revision: rev,
                entity_type: "proxy".into(),
                entity_id: id as u64,
            }))
            .await;
    }
    state
        .events
        .emit("warn", "service", &format!("转发规则已删除:{}", spec.name))
        .await;
    ok(json!({"deleted": true}))
}

#[derive(Deserialize)]
struct BatchReq {
    ids: Vec<i64>,
    action: String, // enable | disable | delete
}

async fn batch_services(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<BatchReq>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let mut results = Vec::new();
    for id in req.ids {
        let r = match req.action.as_str() {
            // 内部直调 inner(本端点已通过 Bearer 校验)
            "enable" => enable_service_inner(&state, id).await,
            "disable" => disable_service_inner(&state, id).await,
            "delete" => delete_service_inner(&state, id).await,
            _ => (9000, format!("未知操作:{}", req.action)),
        };
        results.push(json!({"id": id, "code": r.0, "message": r.1}));
    }
    ok(results)
}

async fn enable_service_inner(state: &Arc<AppState>, id: i64) -> (i32, String) {
    let Some(client_id) = state
        .db
        .get_proxy(id)
        .await
        .ok()
        .flatten()
        .map(|(_, cid, _, _)| cid)
    else {
        return (3001, "not found".into());
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, client_id, mut spec, port_auto)) = state.db.get_proxy(id).await.ok().flatten()
    else {
        return (3001, "not found".into());
    };
    state.db.set_proxy_enabled(id, true).await.ok();
    spec.enabled = true;
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    let client_enabled = state
        .db
        .get_client(&client_id)
        .await
        .ok()
        .flatten()
        .map(|c| c.enabled)
        .unwrap_or(false);
    if !client_enabled {
        state
            .events
            .emit(
                "info",
                "service",
                &format!("转发规则已启用(等待客户端恢复):{}", spec.name),
            )
            .await;
        return (0, "ok".into());
    }
    // 绑定资格失败:回滚 enabled,保持 DB 与运行态一致(与单个 enable 同语义)
    if let Err((code, msg)) = create_bind_checked(state, &client_id, &mut spec, port_auto, id).await
    {
        state.db.set_proxy_enabled(id, false).await.ok();
        return (code, msg);
    }
    crate::runtime::teardown_runtime(state, id as u64).await;
    crate::runtime::spawn_runtime(state, &client_id, &spec, rev).await;
    state
        .events
        .emit("info", "service", &format!("转发规则已启用:{}", spec.name))
        .await;
    (0, "ok".into())
}

async fn disable_service_inner(state: &Arc<AppState>, id: i64) -> (i32, String) {
    let Some((_, client_id, spec, _)) = state.db.get_proxy(id).await.ok().flatten() else {
        return (3001, "not found".into());
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    state.db.set_proxy_enabled(id, false).await.ok();
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    crate::runtime::teardown_runtime(state, id as u64).await;
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceRemove};
        let _ = sess
            .send(ServerMessage::ServiceRemove(ServiceRemove {
                revision: rev,
                entity_type: "proxy".into(),
                entity_id: id as u64,
            }))
            .await;
    }
    state
        .events
        .emit("info", "service", &format!("转发规则已停用:{}", spec.name))
        .await;
    (0, "ok".into())
}

async fn delete_service_inner(state: &Arc<AppState>, id: i64) -> (i32, String) {
    let Some((_, client_id, spec, _)) = state.db.get_proxy(id).await.ok().flatten() else {
        return (3001, "not found".into());
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    crate::runtime::teardown_runtime(state, id as u64).await;
    let _ = state.db.delete_proxy(id).await;
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceRemove};
        let _ = sess
            .send(ServerMessage::ServiceRemove(ServiceRemove {
                revision: rev,
                entity_type: "proxy".into(),
                entity_id: id as u64,
            }))
            .await;
    }
    state
        .events
        .emit("warn", "service", &format!("转发规则已删除:{}", spec.name))
        .await;
    (0, "ok".into())
}

// ---------------- visitors ----------------

async fn list_visitors(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<ListQuery>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let rows = match state.db.list_visitors(q.client_id.as_deref()).await {
        Ok(r) => r,
        Err(e) => return err(500, &format!("{e}")),
    };
    let out: Vec<_> = rows
        .into_iter()
        .map(|(id, client_id, target_client, spec)| {
            json!({
                "id": id,
                "client_id": client_id,
                "target_client_id": target_client,
                "name": spec.name,
                "kind": spec.kind.as_str(),
                "enabled": spec.enabled,
                "bind_addr": spec.bind_addr,
                "bind_port": spec.bind_port,
                "target_proxy_name": spec.target_proxy_name,
                "spec": spec,
            })
        })
        .collect();
    ok(out)
}

#[derive(Deserialize)]
struct CreateVisitorReq {
    client_id: String,
    #[serde(flatten)]
    spec: VisitorSpec,
}

async fn create_visitor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateVisitorReq>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let mut spec = req.spec;
    if spec.name.is_empty() || spec.bind_port == 0 {
        return err(4001, "缺少 name 或 bind_port");
    }
    spec.id = 0;
    // 解析目标代理归属(同名规则可能存在于多个客户端:优先当前在线的客户端,
    // 全部离线时取创建序首个,并在歧义时告警)
    let clients = state.db.list_clients().await.unwrap_or_default();
    let mut candidates: Vec<String> = Vec::new();
    for c in &clients {
        if let Ok(list) = state.db.list_proxies(Some(&c.id)).await {
            if list.iter().any(|(_, _, p)| {
                p.name == spec.target_proxy_name && p.kind.as_str() == spec.kind.as_str()
            }) {
                candidates.push(c.id.clone());
            }
        }
    }
    if candidates.len() > 1 {
        tracing::warn!(
            proxy = %spec.target_proxy_name,
            clients = ?candidates,
            "visitor target proxy name ambiguous; preferring online client"
        );
    }
    let target_client = candidates
        .iter()
        .find(|c| state.sessions.get(c).is_some())
        .or_else(|| candidates.first())
        .cloned();
    let target_proxy_found = target_client.is_some();
    if !target_proxy_found {
        return err(4002, "目标规则不存在(类型须匹配)");
    }
    let lock = state.serial_lock(&req.client_id);
    let _g = lock.lock().await;
    let id = match state
        .db
        .insert_visitor(&req.client_id, &target_client.unwrap(), &spec)
        .await
    {
        Ok(id) => id,
        Err(e) => return err(500, &format!("{e}")),
    };
    let rev = state.db.bump_revision(&req.client_id).await.unwrap_or(0);
    spec.id = id as u64;
    // 下发
    if let Some(sess) = state.sessions.get(&req.client_id) {
        use rh_protocol::message::{ServerMessage, ServiceUpdate, SyncEntity};
        let _ = sess
            .send(ServerMessage::ServiceUpdate(Box::new(ServiceUpdate {
                revision: rev,
                entity: SyncEntity::Visitor(spec.clone()),
            })))
            .await;
    }
    ok(json!({"id": id, "revision": rev}))
}

async fn update_visitor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(mut spec): Json<VisitorSpec>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    // 首读仅为定位属主(串行域);实体在锁内重读,消解与并发管理操作的竞态
    let Some((_, client_id, _, _)) = state
        .db
        .list_visitors(None)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|(vid, ..)| *vid == id)
    else {
        return err(4003, "访问器不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, _, _, _old)) = state
        .db
        .list_visitors(Some(&client_id))
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|(vid, ..)| *vid == id)
    else {
        return err(4003, "访问器不存在");
    };
    spec.id = id as u64;
    // 重新解析目标:与创建同语义(名称 + 类型都须匹配,否则拒绝)
    let clients = state.db.list_clients().await.unwrap_or_default();
    let mut target_client = None;
    for c in &clients {
        if let Ok(list) = state.db.list_proxies(Some(&c.id)).await {
            if list.iter().any(|(_, _, p)| {
                p.name == spec.target_proxy_name && p.kind.as_str() == spec.kind.as_str()
            }) {
                target_client = Some(c.id.clone());
                break;
            }
        }
    }
    let Some(target_client) = target_client else {
        return err(4002, "目标规则不存在(类型须匹配)");
    };
    if let Err(e) = state.db.update_visitor(id, &target_client, &spec).await {
        return err(500, &format!("{e}"));
    }
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceUpdate, SyncEntity};
        let _ = sess
            .send(ServerMessage::ServiceUpdate(Box::new(ServiceUpdate {
                revision: rev,
                entity: SyncEntity::Visitor(spec.clone()),
            })))
            .await;
    }
    ok(json!({"updated": true}))
}

async fn enable_visitor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    toggle_visitor(state, id, true).await
}

async fn disable_visitor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    toggle_visitor(state, id, false).await
}

async fn toggle_visitor(state: Arc<AppState>, id: i64, enabled: bool) -> Response {
    // 首读定位属主;实体在串行域内重读,防与并发删除竞态
    let Some((_, client_id, _, _)) = state
        .db
        .list_visitors(None)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|(vid, ..)| *vid == id)
    else {
        return err(4003, "访问器不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    let Some((_, _, _, mut spec)) = state
        .db
        .list_visitors(Some(&client_id))
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|(vid, ..)| *vid == id)
    else {
        return err(4003, "访问器不存在");
    };
    state.db.set_visitor_enabled(id, enabled).await.ok();
    spec.enabled = enabled;
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceRemove, ServiceUpdate, SyncEntity};
        if enabled {
            let _ = sess
                .send(ServerMessage::ServiceUpdate(Box::new(ServiceUpdate {
                    revision: rev,
                    entity: SyncEntity::Visitor(spec),
                })))
                .await;
        } else {
            let _ = sess
                .send(ServerMessage::ServiceRemove(ServiceRemove {
                    revision: rev,
                    entity_type: "visitor".into(),
                    entity_id: id as u64,
                }))
                .await;
        }
    }
    ok(json!({"enabled": enabled}))
}

async fn delete_visitor(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    // 首读定位属主;实体在串行域内重读,防重复删除/迟到操作复活实体
    let Some((_, client_id, _, _)) = state
        .db
        .list_visitors(None)
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|(vid, ..)| *vid == id)
    else {
        return err(4003, "访问器不存在");
    };
    let lock = state.serial_lock(&client_id);
    let _g = lock.lock().await;
    if !state
        .db
        .list_visitors(Some(&client_id))
        .await
        .unwrap_or_default()
        .iter()
        .any(|(vid, ..)| *vid == id)
    {
        return err(4003, "访问器不存在");
    }
    let rev = state.db.bump_revision(&client_id).await.unwrap_or(0);
    let _ = state.db.delete_visitor(id).await;
    if let Some(sess) = state.sessions.get(&client_id) {
        use rh_protocol::message::{ServerMessage, ServiceRemove};
        let _ = sess
            .send(ServerMessage::ServiceRemove(ServiceRemove {
                revision: rev,
                entity_type: "visitor".into(),
                entity_id: id as u64,
            }))
            .await;
    }
    ok(json!({"deleted": true}))
}

// ---------------- stats / events / metrics ----------------

async fn stats_overview(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    let clients = state.db.list_clients().await.unwrap_or_default();
    let proxies = state.db.list_proxies(None).await.unwrap_or_default();
    let running = state
        .proxies
        .runtimes
        .iter()
        .filter(|e| {
            e.value()
                .state
                .try_read()
                .map(|g| matches!(&*g, ProxyState::Running))
                .unwrap_or(false)
        })
        .count();
    let totals = *state.metrics.totals.lock().unwrap();
    ok(json!({
        "clients_total": clients.len(),
        "clients_online": state.sessions.count_online(),
        "services_total": proxies.len(),
        "services_running": running,
        "conns_current": totals.conns_current,
        "bytes_in": totals.bytes_in,
        "bytes_out": totals.bytes_out,
    }))
}

#[derive(Deserialize)]
struct EventsQuery {
    #[serde(default = "default_limit")]
    limit: i64,
    level: Option<String>,
}

fn default_limit() -> i64 {
    100
}

async fn list_events(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(q): Query<EventsQuery>,
) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    // 钉住合法区间:防 limit<=0(SQLite 视为无限制)或超大值拖库
    let limit = q.limit.clamp(1, 500);
    let rows = match state.db.list_events(limit, q.level.as_deref()).await {
        Ok(r) => r,
        Err(e) => return err(500, &format!("{e}")),
    };
    ok(rows)
}

async fn prometheus_metrics(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    if require_auth(&state, &headers).await.is_err() {
        return unauthorized();
    }
    use prometheus::TextEncoder;
    let encoder = TextEncoder::new();
    let mut buffer = Vec::new();
    if let Ok(mfs) = encoder.encode_to_string(&state.metrics.registry.gather()) {
        buffer.extend_from_slice(mfs.as_bytes());
    }
    (
        StatusCode::OK,
        [("content-type", "text/plain; version=0.0.4")],
        buffer,
    )
        .into_response()
}
