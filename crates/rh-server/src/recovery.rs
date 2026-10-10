//! 启动恢复:资格查询 services.enabled=1 JOIN clients.enabled=1,逐条 bind。

use crate::state::AppState;
use rh_protocol::ProxyKind;
use std::sync::Arc;
use tracing::{info, warn};

pub async fn recover_all(state: &Arc<AppState>) {
    let proxies = state.db.list_proxies(None).await.unwrap_or_default();
    let clients = state.db.list_clients().await.unwrap_or_default();
    let enabled_clients: std::collections::HashSet<String> = clients
        .iter()
        .filter(|c| c.enabled)
        .map(|c| c.id.clone())
        .collect();
    for (id, client_id, mut spec) in proxies {
        if !spec.enabled || !enabled_clients.contains(&client_id) {
            continue;
        }
        crate::db::normalize_spec(&mut spec, state.cfg.limits.default_pool_size);
        // port_auto 被外部占用 → 立即重分配(同事务改库);
        // 固定端口被占不重分配(§4.6):交由 spawn_runtime 进入 bind_error 60s 重试
        let port_auto = state
            .db
            .get_proxy(id)
            .await
            .ok()
            .flatten()
            .map(|(_, _, _, auto)| auto)
            .unwrap_or(false);
        if port_auto && spec.kind.uses_bind_port() && spec.bind_port > 0 {
            let bind_ip = rh_core::net::parse_bind_ip(&spec.bind_ip)
                .unwrap_or(std::net::IpAddr::from([0, 0, 0, 0]));
            let in_use = if spec.kind == ProxyKind::Tcp {
                std::net::TcpListener::bind(std::net::SocketAddr::new(bind_ip, spec.bind_port))
                    .is_err()
            } else {
                std::net::UdpSocket::bind(std::net::SocketAddr::new(bind_ip, spec.bind_port))
                    .is_err()
            };
            if in_use {
                // 重分配
                if let Some(p) = state
                    .proxies
                    .allocate_port(spec.kind.as_str(), bind_ip)
                    .await
                {
                    warn!(
                        proxy = spec.name,
                        old = spec.bind_port,
                        new = p,
                        "PORT_REALLOCATED"
                    );
                    spec.bind_port = p;
                    let _ = state.db.update_proxy(id, &spec, true).await;
                    state.db.bump_revision(&client_id).await.ok();
                    state
                        .events
                        .emit(
                            "warn",
                            "service",
                            &format!("规则 {} 端口重新分配为 {}", spec.name, p),
                        )
                        .await;
                }
            }
        }
        let rev = state
            .db
            .get_client(&client_id)
            .await
            .ok()
            .flatten()
            .map(|c| c.config_revision.max(0) as u64)
            .unwrap_or(0);
        crate::runtime::spawn_runtime(state, &client_id, &spec, rev).await;
        info!(proxy = spec.name, "recovered");
    }
}
