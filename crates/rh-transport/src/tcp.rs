//! TCP/TLS/WS/WSS 载体:共用的拨号、监听与握手管线。
//!
//! 客户端:`connect_stream`(建立首连接)与 `dial_stream`(开一条新流)。
//! 服务端:`listen_stream` + `TcpListenerShim::accept`(完成各层握手后
//! 返回该物理连接的 Multiplexer)。

use crate::ws;
use crate::{
    other_err, ClientTransportConfig, Incoming, Multiplexer, MultiplexerInner,
    ServerTransportConfig,
};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_rustls::TlsConnector;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct TcpListenerShim {
    listener: TcpListener,
    tls: Option<tokio_rustls::TlsAcceptor>,
    ws_upgrade: bool,
    multiplex: bool,
    protocol: crate::Protocol,
}

impl TcpListenerShim {
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    pub async fn accept(&self) -> io::Result<Incoming> {
        let (stream, remote) = self.listener.accept().await?;
        stream.set_nodelay(true).ok();
        set_tcp_keepalive(&stream);
        let mux = match &self.tls {
            Some(acceptor) => {
                let tls_stream = tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(stream))
                    .await
                    .map_err(|_| other_err("tls handshake timeout"))??;
                self.post_tls(Box::new(tls_stream), remote).await?
            }
            None => self.post_tls(Box::new(stream), remote).await?,
        };
        Ok(Incoming {
            mux,
            remote,
            protocol: self.protocol,
        })
    }

    async fn post_tls(
        &self,
        io: crate::BoxedIo,
        _remote: SocketAddr,
    ) -> io::Result<Arc<Multiplexer>> {
        let io = if self.ws_upgrade {
            let io = tokio::time::timeout(HANDSHAKE_TIMEOUT, ws::upgrade_server(io, ws::WS_PATH))
                .await
                .map_err(|_| other_err("ws handshake timeout"))??;
            io
        } else {
            io
        };
        if !self.multiplex {
            return Ok(crate::mux::wrap_stream_as_server(io, false));
        }
        // 混合接入:multiplex 监听允许裸模式客户端。
        // 窥探首 4 字节:RHTP 魔数 = 裸连接(每连接一流);否则按 yamux 处理。
        // 窥探字节必须前传(两种包装都需要)。
        let (prefix, io) = peek_prefix(io).await?;
        if prefix == *b"RHTP" {
            Ok(crate::mux::wrap_stream_as_server(
                crate::PrependedIo::wrap(io, prefix.to_vec()),
                false,
            ))
        } else {
            Ok(crate::mux::wrap_stream_as_server(
                crate::PrependedIo::wrap(io, prefix.to_vec()),
                true,
            ))
        }
    }
}

/// 窥探首 4 字节(带超时,防慢连接占资源);返回字节与可继续读取的流。
async fn peek_prefix(mut io: crate::BoxedIo) -> io::Result<([u8; 4], crate::BoxedIo)> {
    use tokio::io::AsyncReadExt as _;
    let mut b = [0u8; 4];
    tokio::time::timeout(HANDSHAKE_TIMEOUT, io.read_exact(&mut b))
        .await
        .map_err(|_| other_err("peek timeout"))??;
    Ok((b, io))
}

#[allow(unused)]
fn set_tcp_keepalive(stream: &tokio::net::TcpStream) {
    use socket2::{SockRef, TcpKeepalive};
    let s = SockRef::from(stream);
    let ka = TcpKeepalive::new().with_time(Duration::from_secs(30));
    let _ = s.set_tcp_keepalive(&ka);
}

pub async fn listen_stream(cfg: &ServerTransportConfig) -> io::Result<TcpListenerShim> {
    let listener = TcpListener::bind(cfg.bind_addr).await?;
    let tls = if matches!(cfg.protocol, crate::Protocol::Tls | crate::Protocol::Wss) {
        Some(tokio_rustls::TlsAcceptor::from(crate::tls::server_config(
            &cfg.tls,
        )?))
    } else {
        None
    };
    if !cfg.protocol.is_encrypted() {
        tracing::warn!(protocol = %cfg.protocol, "plaintext transport (dev/debug only)");
    }
    Ok(TcpListenerShim {
        listener,
        tls,
        ws_upgrade: matches!(cfg.protocol, crate::Protocol::Ws | crate::Protocol::Wss),
        multiplex: cfg.multiplex,
        protocol: cfg.protocol,
    })
}

/// 客户端:建立一条物理连接并握手。多路复用时返回 yamux mux;
/// 裸模式下连接本体即一条流(AcceptSingle),后续流由 DialPerStream 拨号。
pub async fn connect_stream(cfg: &ClientTransportConfig) -> io::Result<Arc<Multiplexer>> {
    if cfg.multiplex || cfg.protocol.always_muxed() {
        let io = dial_io(cfg).await?;
        Ok(crate::mux::wrap_stream_as_client(io, true))
    } else {
        // 裸模式:开流即拨号
        Ok(Arc::new(Multiplexer {
            inner: MultiplexerInner::DialPerStream { cfg: cfg.clone() },
        }))
    }
}

/// 客户端:为裸模式拨一条新的物理连接(= 一条新流)。
pub async fn dial_stream(cfg: &ClientTransportConfig) -> io::Result<crate::BoxedIo> {
    dial_io(cfg).await
}

async fn dial_io(cfg: &ClientTransportConfig) -> io::Result<crate::BoxedIo> {
    let fut = async {
        let tcp = dial_tcp(cfg).await?;
        match cfg.protocol {
            crate::Protocol::Tcp => {
                Ok::<crate::BoxedIo, io::Error>(Box::new(tcp) as crate::BoxedIo)
            }
            crate::Protocol::Tls | crate::Protocol::Wss => {
                let (rust, verify) = crate::tls::client_config(&cfg.tls)?;
                let connector = TlsConnector::from(rust);
                let name = rustls_pki_types::ServerName::try_from(
                    cfg.tls
                        .server_name
                        .clone()
                        .unwrap_or_else(|| cfg.server_addr.ip().to_string()),
                )
                .map_err(|e| other_err(format!("bad tls server name: {e}")))?
                .to_owned();
                let tls_stream = connector.connect(name, tcp).await?;
                // pin 模式:握手后、发任何数据前比对叶子证书指纹
                if let crate::tls::ClientVerify::Pin(expected) = verify {
                    let certs = tls_stream
                        .get_ref()
                        .1
                        .peer_certificates()
                        .ok_or_else(|| other_err("tls: no peer certificate"))?;
                    let leaf = certs
                        .first()
                        .ok_or_else(|| other_err("tls: empty certificate chain"))?;
                    crate::tls::verify_pin(&expected, leaf.as_ref())?;
                }
                let io: crate::BoxedIo = Box::new(tls_stream);
                if cfg.protocol == crate::Protocol::Wss {
                    ws::client_handshake(
                        io,
                        &cfg.tls
                            .server_name
                            .clone()
                            .unwrap_or_else(|| cfg.server_addr.to_string()),
                        ws::WS_PATH,
                    )
                    .await
                } else {
                    Ok(io)
                }
            }
            crate::Protocol::Ws => {
                ws::client_handshake(Box::new(tcp), &cfg.server_addr.to_string(), ws::WS_PATH).await
            }
            crate::Protocol::Quic | crate::Protocol::Kcp => Err(other_err("use quic/kcp connect")),
        }
    };
    tokio::time::timeout(cfg.connect_timeout, fut)
        .await
        .map_err(|_| other_err("transport connect timeout"))?
}

async fn dial_tcp(cfg: &ClientTransportConfig) -> io::Result<tokio::net::TcpStream> {
    let stream = match &cfg.proxy_url {
        Some(url) => {
            let proxy = crate::proxy_dial::ProxyUrl::parse(url)?;
            crate::proxy_dial::dial_via_proxy(&proxy, cfg.server_addr).await?
        }
        None => tokio::net::TcpStream::connect(cfg.server_addr).await?,
    };
    stream.set_nodelay(true).ok();
    set_tcp_keepalive(&stream);
    Ok(stream)
}
