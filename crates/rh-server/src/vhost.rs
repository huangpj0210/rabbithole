//! vhost:HTTP(域名/子域/locations 路由 + 头改写 + Basic 认证)、
//! HTTPS(SNI 透传)、TCPMux(HTTP CONNECT 复用)。

use crate::state::{AppState, ProxyRuntime, VhostRoute};
use http_body_util::BodyExt as _;
use rh_protocol::forward_flags;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::debug;

// ---------------- HTTP vhost(hyper) ----------------

pub async fn http_vhost_serve(state: Arc<AppState>, listener: tokio::net::TcpListener) {
    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                let st = state.clone();
                tokio::spawn(async move {
                    let _ = sock.set_nodelay(true);
                    if let Err(e) = serve_http(st, sock, peer).await {
                        debug!(error = %e, "http vhost conn");
                    }
                });
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
}

async fn serve_http(state: Arc<AppState>, sock: TcpStream, peer: SocketAddr) -> anyhow::Result<()> {
    use hyper::server::conn::http1;
    use hyper::service::service_fn;

    let st = state.clone();
    let mut builder = http1::Builder::new();
    // 头部读取超时(防 slowloris 未认证挂连接);0 = 不启用
    let header_to = st.cfg.vhost.http_timeout_s;
    if header_to > 0 {
        builder.header_read_timeout(Some(std::time::Duration::from_secs(header_to)));
    }
    let conn = builder
        .timer(hyper_util::rt::TokioTimer::new())
        .serve_connection(
            hyper_util::rt::TokioIo::new(sock),
            service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                let st = st.clone();
                let peer = peer;
                async move {
                    handle_http_request(st, req, peer)
                        .await
                        .map_err(|_e| std::io::Error::other("vhost error"))
                }
            }),
        );
    conn.await?;
    Ok(())
}

type BoxBody = http_body_util::combinators::BoxBody<hyper::body::Bytes, std::io::Error>;

fn full_body(b: &'static [u8]) -> BoxBody {
    http_body_util::Full::new(hyper::body::Bytes::from(b))
        .map_err(|never| match never {})
        .boxed()
}

async fn handle_http_request(
    state: Arc<AppState>,
    req: hyper::Request<hyper::body::Incoming>,
    peer: SocketAddr,
) -> anyhow::Result<hyper::Response<BoxBody>> {
    let host = req
        .headers()
        .get(hyper::header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_lowercase();
    let path = req.uri().path().to_string();

    // 路由:域名 + locations 前缀(最长匹配)
    let routes = state.proxies.vhost_http.read().await.clone();
    let mut best: Option<(&VhostRoute, usize)> = None;
    for r in &routes {
        if r.host != host {
            continue;
        }
        if r.locations.is_empty() {
            if best.map(|(_, l)| l == 0).unwrap_or(true) && best.is_none() {
                best = Some((r, 0));
            }
            continue;
        }
        for loc in &r.locations {
            let l = loc.trim_end_matches('/');
            // 前缀匹配须以路径段为边界:/api 不得命中 /apifoo
            if path == *loc || path.starts_with(&format!("{l}/")) {
                let len = loc.len();
                if best.map(|(_, bl)| len > bl).unwrap_or(true) {
                    best = Some((r, len));
                }
            }
        }
    }
    let Some((route, _)) = best else {
        return Ok(not_found_response(&state));
    };
    // Basic 认证
    if !route.http_user.is_empty() {
        let got = req
            .headers()
            .get(hyper::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        if !check_basic_auth(got.as_deref(), &route.http_user, &route.http_password) {
            let mut resp = hyper::Response::builder()
                .status(401)
                .body(full_body(b"Unauthorized"))?;
            resp.headers_mut().insert(
                hyper::header::WWW_AUTHENTICATE,
                hyper::header::HeaderValue::from_static("Basic realm=\"rabbithole\""),
            );
            return Ok(resp);
        }
    }
    // 上游通道(先组选路再取通道:通道属主须与 START_FORWARD 目标一致)
    let Some(rt) = state.proxies.get(route.proxy_id) else {
        return Ok(not_found_response(&state));
    };
    let rt = crate::data::group_pick_public(&state, &rt)
        .await
        .unwrap_or(rt);
    if !rt.healthy.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(service_unavailable());
    }
    let Some(channel) = crate::data::get_channel(&state, &rt).await else {
        return Ok(service_unavailable());
    };
    let spec = rt.spec.read().await.clone();

    // 头改写 + Host 改写
    let (mut parts, body) = req.into_parts();
    if !route.host_rewrite.is_empty() {
        if let Ok(v) = hyper::header::HeaderValue::from_str(&route.host_rewrite) {
            parts.headers.insert(hyper::header::HOST, v);
        }
    }
    for h in &route.request_headers.set {
        if let Ok(v) = hyper::header::HeaderValue::from_str(&h.value) {
            if let Ok(n) = hyper::header::HeaderName::from_bytes(h.name.as_bytes()) {
                parts.headers.insert(n, v);
            }
        }
    }
    for d in &route.request_headers.del {
        if let Ok(n) = hyper::header::HeaderName::from_bytes(d.as_bytes()) {
            parts.headers.remove(n);
        }
    }
    // X-Forwarded-For / X-Real-IP(HTTP 真实来源)
    if let Ok(v) = hyper::header::HeaderValue::from_str(&peer.ip().to_string()) {
        parts.headers.insert("x-real-ip", v.clone());
        let fwd = parts
            .headers
            .get("x-forwarded-for")
            .and_then(|h| h.to_str().ok())
            .map(|prev| format!("{prev}, {}", peer.ip()))
            .unwrap_or_else(|| peer.ip().to_string());
        if let Ok(v2) = hyper::header::HeaderValue::from_str(&fwd) {
            parts.headers.insert("x-forwarded-for", v2);
        }
    }
    let upstream_req = hyper::Request::from_parts(parts, body);

    // START_FORWARD 指派(最后一帧)
    let mut channel = channel;
    if !send_start_forward(&mut channel, &rt, &spec, &peer.to_string()).await {
        return Ok(service_unavailable());
    }
    let channel = if spec.transport.use_compression {
        rh_transport::compress_stream(channel).await
    } else {
        channel
    };

    // 上游即普通 HTTP/1.1 连接:用 hyper client 桥接
    let upstream = hyper_util::rt::TokioIo::new(channel);
    let resp = hyper::client::conn::http1::handshake(upstream).await?;
    let (mut sender, conn) = resp;
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    let resp = sender.send_request(upstream_req).await?;
    let resp: hyper::Response<BoxBody> = resp
        .map(|b| b.map_err(std::io::Error::other))
        .map(|b| b.boxed());
    // 响应头改写
    let (mut parts, body) = resp.into_parts();
    for h in &route.response_headers.set {
        if let Ok(v) = hyper::header::HeaderValue::from_str(&h.value) {
            if let Ok(n) = hyper::header::HeaderName::from_bytes(h.name.as_bytes()) {
                parts.headers.insert(n, v);
            }
        }
    }
    for d in &route.response_headers.del {
        if let Ok(n) = hyper::header::HeaderName::from_bytes(d.as_bytes()) {
            parts.headers.remove(n);
        }
    }
    Ok(hyper::Response::from_parts(parts, body))
}

fn check_basic_auth(header: Option<&str>, user: &str, pass: &str) -> bool {
    let Some(h) = header else { return false };
    let Some(b64) = h.strip_prefix("Basic ") else {
        return false;
    };
    let decoded = base64_decode(b64).unwrap_or_default();
    let expect = format!("{user}:{pass}");
    rh_transport::constant_time_eq(&decoded, expect.as_bytes())
}

fn not_found_response(state: &Arc<AppState>) -> hyper::Response<BoxBody> {
    if let Some(path) = &state.cfg.vhost.custom_404_page {
        if let Ok(body) = std::fs::read(path) {
            return hyper::Response::builder()
                .status(404)
                .body(
                    http_body_util::Full::new(hyper::body::Bytes::from(body))
                        .map_err(|never| match never {})
                        .boxed(),
                )
                .unwrap();
        }
    }
    hyper::Response::builder()
        .status(404)
        .body(full_body(b"404 page not found"))
        .unwrap()
}

fn service_unavailable() -> hyper::Response<BoxBody> {
    hyper::Response::builder()
        .status(503)
        .body(full_body(b"service unavailable"))
        .unwrap()
}

// ---------------- START_FORWARD 公共 ----------------

pub async fn send_start_forward(
    channel: &mut rh_transport::BoxedIo,
    _rt: &Arc<ProxyRuntime>,
    spec: &rh_protocol::ProxySpec,
    user_src: &str,
) -> bool {
    let mut flags = 0u8;
    if spec.transport.use_compression {
        flags |= forward_flags::COMPRESSED;
    }
    let fwd = rh_protocol::message::StartForward {
        proxy_id: spec.id,
        user_src: user_src.to_string(),
        target_host: spec.target_host.clone(),
        target_port: spec.target_port,
        flags,
        proxy_proto: conv_pp(spec.transport.proxy_proto),
        proxy_name: spec.name.clone(),
    };
    let payload = serde_json::to_vec(&fwd).unwrap();
    rh_protocol::write_frame(channel, rh_protocol::FrameType::START_FORWARD, &payload)
        .await
        .is_ok()
}

// ---------------- HTTPS vhost(SNI) ----------------

pub async fn https_vhost_serve(state: Arc<AppState>, listener: tokio::net::TcpListener) {
    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                let st = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = serve_https(st, sock, peer).await {
                        debug!(error = %e, "https vhost conn");
                    }
                });
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
}

async fn serve_https(
    state: Arc<AppState>,
    sock: TcpStream,
    peer: SocketAddr,
) -> anyhow::Result<()> {
    let _ = sock.set_nodelay(true);
    // peek ClientHello 提取 SNI
    let sock = sock;
    let sni = match peek_sni(&sock).await {
        Ok(Some(s)) => s,
        _ => {
            return Ok(());
        }
    };
    let routes = state.proxies.vhost_https.read().await.clone();
    let route = routes.iter().find(|r| r.host == sni);
    let Some(route) = route else {
        return Ok(());
    };
    let Some(rt) = state.proxies.get(route.proxy_id) else {
        return Ok(());
    };
    let rt = crate::data::group_pick_public(&state, &rt)
        .await
        .unwrap_or(rt);
    if !rt.healthy.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(());
    }
    let spec = rt.spec.read().await.clone();
    let Some(mut channel) = crate::data::get_channel(&state, &rt).await else {
        return Ok(());
    };
    if !send_start_forward(&mut channel, &rt, &spec, &peer.to_string()).await {
        return Ok(());
    }
    let channel = if spec.transport.use_compression {
        rh_transport::compress_stream(channel).await
    } else {
        channel
    };
    let mut channel = channel;
    let mut sock = sock;
    let _ = tokio::io::copy_bidirectional(&mut sock, &mut channel).await;
    Ok(())
}

/// 从 ClientHello 提取 SNI(不解密)。
async fn peek_sni(sock: &TcpStream) -> anyhow::Result<Option<String>> {
    // 16KiB:后量子/多扩展的 ClientHello 可超 4KiB,截断会静默断连
    let mut buf = vec![0u8; 16 * 1024];
    // 循环直到能解析完整 ClientHello 头;必须用 peek(不消费),
    // 后续 copy_bidirectional 需要完整 ClientHello 交给客户端侧 TLS 终结。
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut read = 0;
    loop {
        let n = match sock.peek(&mut buf[read..]).await {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if tokio::time::Instant::now() >= deadline {
                    return Ok(None);
                }
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        // peek 返回当前可读总量(可能小于已累计),用最大值推进
        read = read.max(n);
        if read >= 5 {
            let len = u16::from_be_bytes([buf[3], buf[4]]) as usize + 5;
            if read >= len.min(buf.len()) || read == buf.len() {
                break;
            }
        }
        if read == buf.len() {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            return Ok(None);
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    Ok(parse_sni(&buf[..read]))
}

fn parse_sni(buf: &[u8]) -> Option<String> {
    // TLS record: type(1) ver(2) len(2) handshake...
    if buf.len() < 43 || buf[0] != 0x16 {
        return None;
    }
    let hs_len = u32::from_be_bytes([0, buf[3], buf[4], 0]) as usize; // not used directly
    let _ = hs_len;
    // handshake header: type(1)=0x01 len(3)
    if buf[5] != 0x01 {
        return None;
    }
    // client_version(2) random(32) session_id_len(1) ...
    let mut p = 5 + 4 + 2 + 32;
    if p >= buf.len() {
        return None;
    }
    let sid_len = buf[p] as usize;
    p += 1 + sid_len;
    if p + 2 > buf.len() {
        return None;
    }
    let cs_len = u16::from_be_bytes([buf[p], buf[p + 1]]) as usize;
    p += 2 + cs_len;
    if p >= buf.len() {
        return None;
    }
    let cm_len = buf[p] as usize;
    p += 1 + cm_len;
    if p + 2 > buf.len() {
        return None;
    }
    let ext_len = u16::from_be_bytes([buf[p], buf[p + 1]]) as usize;
    p += 2;
    let ext_end = (p + ext_len).min(buf.len());
    while p + 4 <= ext_end {
        let etype = u16::from_be_bytes([buf[p], buf[p + 1]]);
        let elen = u16::from_be_bytes([buf[p + 2], buf[p + 3]]) as usize;
        p += 4;
        if etype == 0x0000 {
            // SNI: list_len(2) type(1)=0 len(2) name
            if p + 5 <= ext_end && buf[p + 2] == 0 {
                let nlen = u16::from_be_bytes([buf[p + 3], buf[p + 4]]) as usize;
                let start = p + 5;
                if start + nlen <= ext_end {
                    return Some(String::from_utf8_lossy(&buf[start..start + nlen]).to_lowercase());
                }
            }
            return None;
        }
        p += elen;
    }
    None
}

// ---------------- TCPMux(HTTP CONNECT) ----------------

pub async fn tcpmux_serve(
    state: Arc<AppState>,
    listener: tokio::net::TcpListener,
    passthrough: bool,
) {
    loop {
        match listener.accept().await {
            Ok((sock, peer)) => {
                let st = state.clone();
                let pt = passthrough;
                tokio::spawn(async move {
                    if let Err(e) = serve_tcpmux(st, sock, peer, pt).await {
                        debug!(error = %e, "tcpmux conn");
                    }
                });
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(100)).await,
        }
    }
}

async fn serve_tcpmux(
    state: Arc<AppState>,
    mut sock: TcpStream,
    peer: SocketAddr,
    passthrough: bool,
) -> anyhow::Result<()> {
    // 读 CONNECT 请求(终止符之后同批到达的字节须回灌,如 yamux 首帧/TLS ClientHello)
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 512];
    let tail = loop {
        let n = sock.read(&mut chunk).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break buf.split_off(pos + 4);
        }
        if buf.len() > 4096 {
            return Ok(());
        }
    };
    let head = String::from_utf8_lossy(&buf).into_owned();
    let mut lines = head.split("\r\n");
    let first = lines.next().unwrap_or_default().to_string();
    let parts: Vec<&str> = first.split_whitespace().collect();
    if parts.len() < 2 || parts[0] != "CONNECT" {
        sock.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await?;
        return Ok(());
    }
    let hostport = parts[1].to_string();
    let host = hostport.split(':').next().unwrap_or("").to_lowercase();
    // Basic 认证
    let mut user_ok = true;
    {
        let routes = state.proxies.vhost_tcpmux.read().await.clone();
        if let Some(r) = routes.iter().find(|r| r.host == host) {
            if !r.http_user.is_empty() {
                let got = head.lines().find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    if k.trim().eq_ignore_ascii_case("proxy-authorization") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                });
                user_ok = check_basic_auth(got.as_deref(), &r.http_user, &r.http_password);
            }
        }
    }
    if !user_ok {
        sock.write_all(
            b"HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: Basic realm=\"rabbithole\"\r\n\r\n",
        )
        .await?;
        return Ok(());
    }
    // 路由:按 host 匹配 tcpmux 规则
    let routes = state.proxies.vhost_tcpmux.read().await.clone();
    let Some(route) = routes.iter().find(|r| r.host == host) else {
        sock.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await?;
        return Ok(());
    };
    let Some(rt) = state.proxies.get(route.proxy_id) else {
        sock.write_all(b"HTTP/1.1 404 Not Found\r\n\r\n").await?;
        return Ok(());
    };
    let rt = crate::data::group_pick_public(&state, &rt)
        .await
        .unwrap_or(rt);
    let spec = rt.spec.read().await.clone();
    if passthrough {
        let Some(mut channel) = crate::data::get_channel(&state, &rt).await else {
            sock.write_all(b"HTTP/1.1 503 Service Unavailable\r\n\r\n")
                .await?;
            return Ok(());
        };
        // 透传模式:CONNECT 200 后整条流直通客户端,由客户端处理多路复用
        sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
            .await?;
        let mut flags = 0u8;
        if spec.transport.use_compression {
            flags |= forward_flags::COMPRESSED;
        }
        flags |= forward_flags::TCPMUX_PASSTHROUGH;
        let fwd = rh_protocol::message::StartForward {
            proxy_id: spec.id,
            user_src: peer.to_string(),
            target_host: spec.target_host.clone(),
            target_port: spec.target_port,
            flags,
            proxy_proto: conv_pp(spec.transport.proxy_proto),
            proxy_name: spec.name.clone(),
        };
        rh_protocol::write_frame(
            &mut channel,
            rh_protocol::FrameType::START_FORWARD,
            &serde_json::to_vec(&fwd)?,
        )
        .await?;
        let channel = if spec.transport.use_compression {
            rh_transport::compress_stream(channel).await
        } else {
            channel
        };
        let mut channel = channel;
        // CONNECT 后已读入的用户数据(如 yamux 首帧)前转进通道
        if !tail.is_empty() && channel.write_all(&tail).await.is_err() {
            return Ok(());
        }
        let _ = tokio::io::copy_bidirectional(&mut sock, &mut channel).await;
        return Ok(());
    }
    // 标准模式:服务端在用户连接上终结 yamux,每个入站流 = 一次独立数据通道转发
    sock.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    let mux =
        rh_transport::mux::tcpmux_server_mux(rh_transport::PrependedIo::wrap(Box::new(sock), tail));
    loop {
        let stream = match mux.accept().await {
            Ok(s) => s,
            Err(_) => break,
        };
        let st2 = state.clone();
        let rt2 = rt.clone();
        let peer2 = peer;
        tokio::spawn(async move {
            // 每个流:新通道 + 指派
            if let Some(mut ch) = crate::data::get_channel(&st2, &rt2).await {
                let spec = rt2.spec.read().await.clone();
                let ch = if send_start_forward(&mut ch, &rt2, &spec, &peer2.to_string()).await {
                    if spec.transport.use_compression {
                        rh_transport::compress_stream(ch).await
                    } else {
                        ch
                    }
                } else {
                    return;
                };
                let mut ch = ch;
                let mut stream = stream;
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut ch).await;
            }
        });
    }
    Ok(())
}

/// 最小 base64 解码(标准字母表)。
fn base64_decode(s: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut vals = Vec::with_capacity(s.len());
    for c in s.chars() {
        if c == '=' || c == '\n' || c == '\r' {
            continue;
        }
        let v = TABLE.iter().position(|&t| t as char == c)?;
        vals.push(v as u8);
    }
    let mut out = Vec::with_capacity(vals.len() * 3 / 4);
    for chunk in vals.chunks(4) {
        match chunk.len() {
            4 => {
                out.push((chunk[0] << 2) | (chunk[1] >> 4));
                out.push((chunk[1] << 4) | (chunk[2] >> 2));
                out.push((chunk[2] << 6) | chunk[3]);
            }
            3 => {
                out.push((chunk[0] << 2) | (chunk[1] >> 4));
                out.push((chunk[1] << 4) | (chunk[2] >> 2));
            }
            2 => {
                out.push((chunk[0] << 2) | (chunk[1] >> 4));
            }
            _ => {}
        }
    }
    Some(out)
}

fn conv_pp(v: rh_protocol::ProxyProtoVersion) -> rh_protocol::message::ProxyProto {
    match v {
        rh_protocol::ProxyProtoVersion::None => rh_protocol::message::ProxyProto::None,
        rh_protocol::ProxyProtoVersion::V1 => rh_protocol::message::ProxyProto::V1,
        rh_protocol::ProxyProtoVersion::V2 => rh_protocol::message::ProxyProto::V2,
    }
}
