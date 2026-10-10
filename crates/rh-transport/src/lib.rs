//! rh-transport:统一传输抽象。
//!
//! - 物理载体:tcp / tls / websocket / wss / quic / kcp。
//! - 多路复用:quic/kcp 天然多流;tcp/tls/ws 可选 yamux;不复用时每流一连接。
//! - 客户端可经 HTTP CONNECT 或 SOCKS5 代理拨号(仅面向 TCP 的载体)。
//! - snappy 流压缩包装(代理级 use_compression)。
//!
//! 连接/流分离见 [`Multiplexer`]:客户端 `open_stream`,服务端 `accept`,
//! 与设计 §10 的预留边界一致(QUIC 场景一连接多流,裸 TCP 场景一流一连接)。

pub mod compress;
pub mod kcp;
pub mod mux;
pub use mux::{wrap_stream_as_client, wrap_stream_as_server};
pub mod proxy_dial;
pub mod quic;
pub mod tcp;
pub mod tls;
pub mod ws;

use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncWrite};

pub use compress::compress_stream;

/// 底层 IO 对象:实现 AsyncRead + AsyncWrite + Send + Unpin 即可。
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

pub type BoxedIo = Box<dyn Io>;

/// 在真实流之前回放一段残留字节。
/// read_frame 可能一次读入多帧;换手给下一处理者时必须把
/// 已读入但未消费的字节前传,否则同批到达的后续帧丢失。
pub struct PrependedIo {
    inner: BoxedIo,
    prefix: Vec<u8>,
    pos: usize,
}

impl PrependedIo {
    pub fn wrap(inner: BoxedIo, prefix: Vec<u8>) -> BoxedIo {
        Box::new(Self {
            inner,
            prefix,
            pos: 0,
        })
    }
}

impl AsyncRead for PrependedIo {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.pos < this.prefix.len() {
            let n = (this.prefix.len() - this.pos).min(buf.remaining());
            buf.put_slice(&this.prefix[this.pos..this.pos + n]);
            this.pos += n;
            return std::task::Poll::Ready(Ok(()));
        }
        std::pin::Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for PrependedIo {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let inner = &mut self.get_mut().inner;
        std::pin::Pin::new(inner).poll_write(cx, buf)
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let inner = &mut self.get_mut().inner;
        std::pin::Pin::new(inner).poll_flush(cx)
    }
    fn poll_shutdown(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let inner = &mut self.get_mut().inner;
        std::pin::Pin::new(inner).poll_shutdown(cx)
    }
}

/// 传输协议载体。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    /// 明文 TCP(仅建议本机调试;公网使用 tls+)。
    Tcp,
    Tls,
    Ws,
    Wss,
    Quic,
    Kcp,
}

impl Protocol {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "tcp" => Some(Protocol::Tcp),
            "tls" => Some(Protocol::Tls),
            "ws" => Some(Protocol::Ws),
            "wss" => Some(Protocol::Wss),
            "quic" => Some(Protocol::Quic),
            "kcp" => Some(Protocol::Kcp),
            _ => None,
        }
    }
    pub fn is_tcp_based(&self) -> bool {
        matches!(
            self,
            Protocol::Tcp | Protocol::Tls | Protocol::Ws | Protocol::Wss
        )
    }
    pub fn is_encrypted(&self) -> bool {
        matches!(self, Protocol::Tls | Protocol::Wss | Protocol::Quic)
    }
    /// 该载体是否总是多路复用(quic/kcp 是;其余取决于 multiplex 配置)。
    pub fn always_muxed(&self) -> bool {
        matches!(self, Protocol::Quic | Protocol::Kcp)
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Protocol::Tcp => "tcp",
            Protocol::Tls => "tls",
            Protocol::Ws => "ws",
            Protocol::Wss => "wss",
            Protocol::Quic => "quic",
            Protocol::Kcp => "kcp",
        };
        f.write_str(s)
    }
}

/// TLS 客户端选项。
#[derive(Debug, Clone, Default)]
pub struct TlsClientOptions {
    /// 信任 CA(PEM 字节);None 时仅能配合 pin 或 insecure。
    pub trusted_ca: Option<Vec<u8>>,
    /// SNI / 校验名;None 用地址主机名。
    pub server_name: Option<String>,
    /// 跳过校验(仅测试)。
    pub insecure_skip_verify: bool,
    /// 固定服务端叶子证书 SHA-256 指纹(hex)。设置时优先于 CA 链校验。
    pub pin_sha256: Option<String>,
}

/// TLS 服务端选项。
#[derive(Debug, Clone, Default)]
pub struct TlsServerOptions {
    pub cert_pem: Option<Vec<u8>>,
    pub key_pem: Option<Vec<u8>>,
    /// 无 cert/key 时自动生成自签证书,以 `persist_prefix` 落盘复用。
    pub persist_prefix: Option<std::path::PathBuf>,
}

/// 客户端传输配置。
#[derive(Debug, Clone)]
pub struct ClientTransportConfig {
    pub protocol: Protocol,
    pub server_addr: SocketAddr,
    /// tcp/tls/ws/wss 是否启用 yamux 多路复用。
    pub multiplex: bool,
    pub tls: TlsClientOptions,
    pub proxy_url: Option<String>,
    /// ALPN。
    pub alpn: Vec<String>,
    /// 拨号总超时。
    pub connect_timeout: std::time::Duration,
}

impl Default for ClientTransportConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::Tcp,
            server_addr: "127.0.0.1:7000".parse().unwrap(),
            multiplex: false,
            tls: TlsClientOptions::default(),
            proxy_url: None,
            alpn: vec!["rhtp".to_string()],
            connect_timeout: std::time::Duration::from_secs(10),
        }
    }
}

/// 服务端传输配置(单个监听点)。
#[derive(Debug, Clone)]
pub struct ServerTransportConfig {
    pub protocol: Protocol,
    pub bind_addr: SocketAddr,
    pub multiplex: bool,
    pub tls: TlsServerOptions,
    pub alpn: Vec<String>,
}

impl Default for ServerTransportConfig {
    fn default() -> Self {
        Self {
            protocol: Protocol::Tcp,
            bind_addr: "0.0.0.0:7000".parse().unwrap(),
            multiplex: false,
            tls: TlsServerOptions::default(),
            alpn: vec!["rhtp".to_string()],
        }
    }
}

/// 服务端 accept 出的一个连接(多路复用器)。
pub struct Incoming {
    pub mux: Arc<Multiplexer>,
    pub remote: SocketAddr,
    /// 底层协议(诊断)。
    pub protocol: Protocol,
}

enum ListenerInner {
    Tcp(tcp::TcpListenerShim),
    Quic(quic::QuicListener),
    Kcp(kcp::KcpListener),
}

/// 服务端监听器:`accept()` 产出 [`Incoming`]。
pub struct TransportListener {
    inner: ListenerInner,
    pub local_addr: SocketAddr,
    pub protocol: Protocol,
}

/// 多路复用器:客户端侧开流,服务端侧收流。
pub struct Multiplexer {
    inner: MultiplexerInner,
}

pub(crate) enum MultiplexerInner {
    /// 裸模式客户端:开流 = 新建整条物理连接。
    DialPerStream {
        cfg: ClientTransportConfig,
    },
    /// 裸模式服务端:accept 恰好一次(该物理连接本身)。
    AcceptSingle {
        stream: tokio::sync::Mutex<Option<BoxedIo>>,
    },
    Yamux(mux::YamuxMultiplexer),
    Quic(quic::QuicConnection),
}

impl Multiplexer {
    /// 客户端:打开一个新流(多路复用或新连接)。
    pub async fn open_stream(&self) -> std::io::Result<BoxedIo> {
        match &self.inner {
            MultiplexerInner::DialPerStream { cfg } => tcp::dial_stream(cfg).await,
            MultiplexerInner::AcceptSingle { .. } => {
                Err(other_err("open_stream on server-side single multiplexer"))
            }
            MultiplexerInner::Yamux(m) => m.open_stream().await,
            MultiplexerInner::Quic(q) => q.open_stream().await,
        }
    }

    /// 服务端:接受一个新流。裸模式 = 返回该物理连接(仅一次)。
    pub async fn accept(&self) -> std::io::Result<BoxedIo> {
        match &self.inner {
            MultiplexerInner::DialPerStream { .. } => {
                Err(other_err("accept on client multiplexer"))
            }
            MultiplexerInner::AcceptSingle { stream } => {
                let mut guard = stream.lock().await;
                guard.take().ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::NotConnected,
                        "physical connection stream already taken",
                    )
                })
            }
            MultiplexerInner::Yamux(m) => m.accept_stream().await,
            MultiplexerInner::Quic(q) => q.accept_stream().await,
        }
    }

    /// 连接是否已死(无法再产生新流)。
    pub async fn is_dead(&self) -> bool {
        match &self.inner {
            MultiplexerInner::Yamux(m) => m.is_dead(),
            MultiplexerInner::Quic(q) => q.is_dead(),
            _ => false,
        }
    }
}

pub(crate) fn other_err(msg: impl Into<String>) -> std::io::Error {
    std::io::Error::other(msg.into())
}

/// 常数时间字节比较(用于指纹/密钥比对)。
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 最小 base64 编码(标准字母表,含 padding)。
pub fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        out.push(TABLE[(b[0] >> 2) as usize] as char);
        out.push(TABLE[(((b[0] & 0x03) << 4) | (b[1] >> 4)) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(((b[1] & 0x0f) << 2) | (b[2] >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(b[2] & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// 服务端:按配置监听。
pub async fn listen(cfg: &ServerTransportConfig) -> std::io::Result<TransportListener> {
    let local_addr;
    let inner = match cfg.protocol {
        Protocol::Tcp | Protocol::Tls | Protocol::Ws | Protocol::Wss => {
            let l = tcp::listen_stream(cfg).await?;
            local_addr = l.local_addr()?;
            ListenerInner::Tcp(l)
        }
        Protocol::Quic => {
            let e = quic::listen(cfg)?;
            local_addr = e.local;
            ListenerInner::Quic(e)
        }
        Protocol::Kcp => {
            let l = kcp::listen(cfg).await?;
            local_addr = l.local_addr();
            ListenerInner::Kcp(l)
        }
    };
    Ok(TransportListener {
        inner,
        local_addr,
        protocol: cfg.protocol,
    })
}

impl TransportListener {
    pub async fn accept(&mut self) -> std::io::Result<Incoming> {
        match &mut self.inner {
            ListenerInner::Tcp(l) => l.accept().await,
            ListenerInner::Quic(e) => quic::accept(e).await,
            ListenerInner::Kcp(l) => l.accept().await,
        }
    }
}

/// 客户端:建立连接并返回其多路复用器。
/// 控制流 = 之后在 mux 上打开的第一个流。
pub async fn connect(cfg: &ClientTransportConfig) -> std::io::Result<Arc<Multiplexer>> {
    match cfg.protocol {
        Protocol::Tcp | Protocol::Tls | Protocol::Ws | Protocol::Wss => {
            tcp::connect_stream(cfg).await
        }
        Protocol::Quic => quic::connect(cfg).await,
        Protocol::Kcp => kcp::connect(cfg).await,
    }
}
