//! visitor 中继:stcp/sudp 访问端在服务端的中继配对与 xtcp 协调入口。
//!
//! 配对模型:服务端为一次访问在两侧各签发一条数据通道
//! (visitor 侧 VisitorRelay / 代理侧 RelayProxy),到达后:
//! - TCP:代理侧写 START_FORWARD(自包含目标)后裸流,visitor 侧写
//!   START_VISITOR_RELAY 后裸流,服务端 splice 两条流;
//! - UDP(SUDP):两侧均为 UDP_PACKET 帧通道,帧对帧转发;
//! - XTCP:先打洞协调,失败回退以上中继。

use crate::session::{RequestPurpose, Session};
use crate::state::AppState;
use rh_protocol::message::{ChannelEntity, StartForward, StartVisitorRelay, UserConnect};
use rh_protocol::{forward_flags, FrameType, ProxyKind, ProxySpec, VisitorSpec};
use rh_transport::BoxedIo;
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

/// USER_CONNECT:visitor 本地新连接(stcp/xtcp TCP;sudp 首包触发)。
pub async fn on_user_connect(state: &Arc<AppState>, sess: &Arc<Session>, u: UserConnect) {
    let visitors = state
        .db
        .list_visitors(Some(&sess.client_id))
        .await
        .unwrap_or_default();
    let Some((_vid, _v_client, v_target_client, vspec)) = visitors
        .into_iter()
        .find(|(id, _, _, s)| *id as u64 == u.visitor_id && s.enabled)
    else {
        return;
    };
    let proxies = state
        .db
        .list_proxies(Some(&v_target_client))
        .await
        .unwrap_or_default();
    let Some((_pid, _pcid, pspec)) = proxies
        .into_iter()
        .find(|(_, _, s)| s.name == vspec.target_proxy_name && s.enabled)
    else {
        warn!(visitor = vspec.name, "target proxy not found/enabled");
        return;
    };
    if !secret_and_allow_ok(state, sess, &pspec, &vspec).await {
        return;
    }
    let Some(target_sess) = state.sessions.get(&v_target_client) else {
        warn!(visitor = vspec.name, target = %v_target_client, "target client offline");
        return;
    };

    if u.udp {
        relay_udp(state, sess, &vspec, &pspec, &target_sess).await;
        return;
    }
    if vspec.kind == rh_protocol::VisitorKind::Xtcp {
        // 同一 user_conn_id 的重发 UserConnect = 客户端打洞失败的回退信号:
        // 首次尝试打洞;重发直接走服务端中继。
        let already = state
            .nathole
            .mark_attempted(&sess.session_id, u.user_conn_id)
            .await;
        tracing::info!(
            visitor = vspec.name,
            conn = u.user_conn_id,
            reissue = already,
            "xtcp user connect: punch or relay"
        );
        if !already {
            let fallback =
                crate::nathole::start(state, sess, &vspec, &pspec, &target_sess, u.user_conn_id)
                    .await;
            if !fallback {
                return; // 打洞协调已发起
            }
        }
    }
    tracing::info!(
        visitor = vspec.name,
        conn = u.user_conn_id,
        "visitor relay established"
    );
    relay_tcp(
        state,
        sess,
        &vspec,
        &pspec,
        &target_sess,
        u.user_conn_id,
        u.src_addr.clone(),
    )
    .await;
}

async fn secret_and_allow_ok(
    state: &Arc<AppState>,
    sess: &Arc<Session>,
    pspec: &ProxySpec,
    vspec: &rh_protocol::VisitorSpec,
) -> bool {
    let Some(secret_cfg) = pspec.secret_cfg() else {
        return false;
    };
    if !secret_cfg.secret_key.is_empty() && secret_cfg.secret_key != vspec.secret_key {
        warn!(visitor = vspec.name, "visitor secret mismatch");
        return false;
    }
    if !secret_cfg.allow_users.is_empty() {
        let my_name = state
            .db
            .get_client(&sess.client_id)
            .await
            .ok()
            .flatten()
            .map(|c| c.name)
            .unwrap_or_default();
        if !secret_cfg.allow_users.contains(&my_name) {
            warn!(visitor = vspec.name, "visitor client not allowed");
            return false;
        }
    }
    true
}

/// TCP 中继。
async fn relay_tcp(
    state: &Arc<AppState>,
    sess: &Arc<Session>,
    vspec: &VisitorSpec,
    pspec: &ProxySpec,
    target_sess: &Arc<Session>,
    user_conn_id: u64,
    src_addr: String,
) {
    // 压缩必须两端一致(服务端裸 splice):任一侧未开启则整体不压缩
    let compress = vspec.use_compression && pspec.transport.use_compression;
    let Some((_visitor_rid, _proxy_rid, vr, pr)) = state
        .issue_relay_pair(
            sess,
            target_sess,
            ChannelEntity::Visitor {
                visitor_id: vspec.id,
                user_conn_id,
            },
            RequestPurpose::VisitorRelay {
                visitor_id: vspec.id,
                user_conn_id,
                compress,
            },
            ChannelEntity::Proxy { proxy_id: pspec.id },
            RequestPurpose::RelayProxy {
                proxy_id: pspec.id,
                compress,
            },
        )
        .await
    else {
        return;
    };
    let st = state.clone();
    let pspec2 = pspec.clone();
    let vid_sess = sess.session_id.clone();
    let pid_sess = target_sess.session_id.clone();
    tokio::spawn(async move {
        // 通道签发后 ≤15s 内必须两侧都到达;超时清理配对槽位,防任务与表项泄漏
        let visitor_stream = match tokio::time::timeout(Duration::from_secs(15), vr).await {
            Ok(Ok(s)) => s,
            _ => {
                cleanup_pair(&st, &vid_sess, _visitor_rid, &pid_sess, _proxy_rid).await;
                return;
            }
        };
        let proxy_stream = match tokio::time::timeout(Duration::from_secs(15), pr).await {
            Ok(Ok(s)) => s,
            _ => {
                cleanup_pair(&st, &vid_sess, _visitor_rid, &pid_sess, _proxy_rid).await;
                return;
            }
        };
        splice_relay(st, pspec2, src_addr, proxy_stream, visitor_stream).await;
    });
}

/// 超时/失败路径:移除两条配对表登记(deliver_pair 只处理"抵达"路径)。
async fn cleanup_pair(
    state: &Arc<AppState>,
    vid_sess: &str,
    visitor_rid: u64,
    pid_sess: &str,
    proxy_rid: u64,
) {
    let mut pairs = state.relay_pairs.lock().await;
    pairs.remove(&(vid_sess.to_string(), visitor_rid));
    pairs.remove(&(pid_sess.to_string(), proxy_rid));
}

/// SUDP 帧中继(两侧常驻通道)。
async fn relay_udp(
    state: &Arc<AppState>,
    sess: &Arc<Session>,
    vspec: &VisitorSpec,
    pspec: &ProxySpec,
    target_sess: &Arc<Session>,
) {
    // 已有常驻中继则复用(每 visitor 一条)。
    // 先占位后签发:登记与检查之间不得隔 await,否则并发首包可建出第二条
    {
        let mut relays = state.udp_relays.lock().await;
        if relays.contains(&vspec.id) {
            return;
        }
        relays.insert(vspec.id);
    }
    // 压缩两端一致(同 relay_tcp)
    let compress = vspec.use_compression && pspec.transport.use_compression;
    let Some((_visitor_rid, _proxy_rid, vr, pr)) = state
        .issue_relay_pair(
            sess,
            target_sess,
            ChannelEntity::Visitor {
                visitor_id: vspec.id,
                user_conn_id: 0,
            },
            RequestPurpose::VisitorRelay {
                visitor_id: vspec.id,
                user_conn_id: 0,
                compress,
            },
            ChannelEntity::Proxy { proxy_id: pspec.id },
            RequestPurpose::RelayProxy {
                proxy_id: pspec.id,
                compress,
            },
        )
        .await
    else {
        state.udp_relays.lock().await.remove(&vspec.id);
        return;
    };
    let st = state.clone();
    let vid = vspec.id;
    tokio::spawn(async move {
        // 通道签发后 ≤15s 内必须到达;否则清理 relay 槽位避免死占
        let a = match tokio::time::timeout(Duration::from_secs(15), pr).await {
            Ok(Ok(s)) => s,
            _ => {
                st.udp_relays.lock().await.remove(&vid);
                return;
            }
        };
        let b = match tokio::time::timeout(Duration::from_secs(15), vr).await {
            Ok(Ok(s)) => s,
            _ => {
                st.udp_relays.lock().await.remove(&vid);
                return;
            }
        };
        let mut a = a;
        let mut b = crate::data::CountedUserIo::new(b, st.metrics.clone());
        let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
        st.udp_relays.lock().await.remove(&vid);
    });
}

/// visitor 侧通道抵达(route_data_stream 调用):
/// TCP 写 START_VISITOR_RELAY(最后一帧)→ 交付配对;UDP(user_conn_id=0)直接交付。
/// compress 来自签发时的两端一致性决策(purpose 携带,免再查库)。
pub async fn on_visitor_channel(
    state: &Arc<AppState>,
    session_id: &str,
    request_id: u64,
    visitor_id: u64,
    user_conn_id: u64,
    compress: bool,
    mut stream: BoxedIo,
) {
    if user_conn_id != 0 {
        let mut flags = 0u8;
        if compress {
            flags |= forward_flags::COMPRESSED;
        }
        let relay = StartVisitorRelay {
            visitor_id,
            user_conn_id,
            flags,
        };
        let payload = serde_json::to_vec(&relay).unwrap();
        if rh_protocol::write_frame(&mut stream, FrameType::START_VISITOR_RELAY, &payload)
            .await
            .is_err()
        {
            return;
        }
    }
    deliver_pair(state, session_id, request_id, stream).await;
}

/// 代理侧中继通道抵达:TCP 写 START_FORWARD 后交付;UDP 直接交付。
pub async fn on_relay_proxy_channel(
    state: &Arc<AppState>,
    session_id: &str,
    request_id: u64,
    proxy_id: u64,
    src_hint: &str,
    compress: bool,
    mut stream: BoxedIo,
) {
    let Some(rt) = state.proxies.get(proxy_id) else {
        warn!(proxy_id, "relay proxy channel: proxy runtime missing");
        return;
    };
    let pspec = rt.spec.read().await.clone();
    if pspec.kind == ProxyKind::Sudp {
        // UDP 语义:无指派帧,直接交付(客户端按 UDP 通道处理)
        deliver_pair(state, session_id, request_id, stream).await;
        return;
    }
    let mut flags = 0u8;
    if compress {
        flags |= forward_flags::COMPRESSED;
    }
    let fwd = StartForward {
        proxy_id: pspec.id,
        user_src: src_hint.to_string(),
        target_host: pspec.target_host.clone(),
        target_port: pspec.target_port,
        flags,
        proxy_proto: conv_proxy_proto(pspec.transport.proxy_proto),
        proxy_name: pspec.name.clone(),
    };
    let payload = serde_json::to_vec(&fwd).unwrap();
    if rh_protocol::write_frame(&mut stream, FrameType::START_FORWARD, &payload)
        .await
        .is_err()
    {
        warn!(proxy_id, "relay proxy channel: start forward write failed");
        return;
    }
    tracing::debug!(proxy_id, request_id, "relay proxy channel assigned");
    deliver_pair(state, session_id, request_id, stream).await;
}

async fn deliver_pair(state: &Arc<AppState>, session_id: &str, request_id: u64, stream: BoxedIo) {
    let slot = state
        .relay_pairs
        .lock()
        .await
        .remove(&(session_id.to_string(), request_id));
    if let Some(tx) = slot {
        let _ = tx.send(stream);
    } else {
        warn!(
            session_id,
            request_id, "relay pair slot missing (expired or collision)"
        );
    }
}

/// 中继拼管(TCP):裸双向搬运。指派帧(START_FORWARD/START_VISITOR_RELAY)
/// 已在两侧通道抵达时写入并带压缩 flags,压缩由两端客户端各自完成。
async fn splice_relay(
    state: Arc<AppState>,
    pspec: ProxySpec,
    src_addr: String,
    proxy_stream: BoxedIo,
    visitor_stream: BoxedIo,
) {
    let _ = (&pspec, src_addr);
    let mut a = proxy_stream;
    // 计量取 visitor 侧(用户侧口径,与 tcp/vhost 一致)
    let mut b = crate::data::CountedUserIo::new(visitor_stream, state.metrics.clone());
    let _ = tokio::io::copy_bidirectional(&mut a, &mut b).await;
}

fn conv_proxy_proto(v: rh_protocol::ProxyProtoVersion) -> rh_protocol::message::ProxyProto {
    match v {
        rh_protocol::ProxyProtoVersion::None => rh_protocol::message::ProxyProto::None,
        rh_protocol::ProxyProtoVersion::V1 => rh_protocol::message::ProxyProto::V1,
        rh_protocol::ProxyProtoVersion::V2 => rh_protocol::message::ProxyProto::V2,
    }
}
