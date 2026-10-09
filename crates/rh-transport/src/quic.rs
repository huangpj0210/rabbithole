//! QUIC 载体(quinn):原生多流,一连接多流;ALPN "rhtp"。

use crate::{
    other_err, BoxedIo, ClientTransportConfig, Incoming, Multiplexer, MultiplexerInner,
    ServerTransportConfig,
};
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const QUIC_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

pub fn listen(cfg: &ServerTransportConfig) -> io::Result<quinn::Endpoint> {
    let tls = crate::tls::server_config_with_alpn(&cfg.tls, &cfg.alpn)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(tls)
            .map_err(|e| other_err(format!("quic server config: {e}")))?,
    ));
    let endpoint = quinn::Endpoint::server(server_config, cfg.bind_addr)
        .map_err(|e| other_err(format!("quic bind: {e}")))?;
    Ok(endpoint)
}

pub async fn accept(endpoint: &quinn::Endpoint) -> io::Result<Incoming> {
    let incoming = endpoint
        .accept()
        .await
        .ok_or_else(|| other_err("quic listener closed"))?;
    let conn = tokio::time::timeout(QUIC_HANDSHAKE_TIMEOUT, incoming)
        .await
        .map_err(|_| other_err("quic handshake timeout"))?
        .map_err(|e| other_err(format!("quic handshake: {e}")))?;
    let remote = conn.remote_address();
    Ok(Incoming {
        mux: Arc::new(Multiplexer {
            inner: MultiplexerInner::Quic(QuicConnection::new(conn)),
        }),
        remote,
        protocol: crate::Protocol::Quic,
    })
}

pub async fn connect(cfg: &ClientTransportConfig) -> io::Result<Arc<Multiplexer>> {
    let (tls, verify) = crate::tls::client_config_with_alpn(&cfg.tls, &cfg.alpn)?;
    let qcfg = quinn::ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(tls)
            .map_err(|e| other_err(format!("quic client config: {e}")))?,
    ));
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
        .map_err(|e| other_err(format!("quic client bind: {e}")))?;
    endpoint.set_default_client_config(qcfg);
    let name = cfg
        .tls
        .server_name
        .clone()
        .unwrap_or_else(|| cfg.server_addr.ip().to_string());
    let connecting = tokio::time::timeout(cfg.connect_timeout, async {
        endpoint.connect(cfg.server_addr, &name)
    })
    .await
    .map_err(|_| other_err("quic connect timeout"))?
    .map_err(|e| other_err(format!("quic connect: {e}")))?;
    let conn = tokio::time::timeout(cfg.connect_timeout, connecting)
        .await
        .map_err(|_| other_err("quic handshake timeout"))?
        .map_err(|e| other_err(format!("quic handshake: {e}")))?;
    // pin 模式:握手后比对叶子证书指纹
    if let crate::tls::ClientVerify::Pin(expected) = verify {
        let identity = conn
            .peer_identity()
            .ok_or_else(|| other_err("quic: no peer certificate"))?;
        let certs = identity
            .downcast::<Vec<rustls_pki_types::CertificateDer<'static>>>()
            .map_err(|_| other_err("quic: unexpected peer identity type"))?;
        let leaf = certs
            .first()
            .ok_or_else(|| other_err("quic: empty certificate chain"))?;
        crate::tls::verify_pin(&expected, leaf.as_ref())?;
    }
    Ok(Arc::new(Multiplexer {
        inner: MultiplexerInner::Quic(QuicConnection::new(conn)),
    }))
}

/// quinn 连接包装:开流/收流 + 关闭状态位。
pub struct QuicConnection {
    conn: quinn::Connection,
    dead: Arc<AtomicBool>,
}

impl QuicConnection {
    pub fn new(conn: quinn::Connection) -> Self {
        let dead = Arc::new(AtomicBool::new(false));
        let watcher_conn = conn.clone();
        let watcher_dead = dead.clone();
        tokio::spawn(async move {
            let _ = watcher_conn.closed().await;
            watcher_dead.store(true, Ordering::Relaxed);
        });
        Self { conn, dead }
    }

    pub async fn open_stream(&self) -> io::Result<BoxedIo> {
        let (send, recv) = self
            .conn
            .open_bi()
            .await
            .map_err(|e| other_err(format!("quic open_bi: {e}")))?;
        Ok(Box::new(BiStream { send, recv }))
    }

    pub async fn accept_stream(&self) -> io::Result<BoxedIo> {
        match self.conn.accept_bi().await {
            Ok((send, recv)) => Ok(Box::new(BiStream { send, recv })),
            Err(e) => Err(other_err(format!("quic accept_bi: {e}"))),
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }
}

/// quinn 双向流包装为字节流(读 RecvStream,写 SendStream)。
pub struct BiStream {
    send: quinn::SendStream,
    recv: quinn::RecvStream,
}

impl AsyncRead for BiStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.recv).poll_read(cx, buf)
    }
}

impl AsyncWrite for BiStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.send)
            .poll_write(cx, buf)
            .map_err(|e| other_err(format!("quic write: {e}")))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.send.finish() {
            Ok(()) => Poll::Ready(Ok(())),
            Err(e) => Poll::Ready(Err(other_err(format!("quic finish: {e}")))),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{listen, ClientTransportConfig, Protocol, ServerTransportConfig};

    #[tokio::test]
    async fn quic_connect_and_stream() {
        let (cert, key) = crate::tls::auto_cert(&Default::default()).unwrap();
        let server_cfg = ServerTransportConfig {
            protocol: Protocol::Quic,
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            multiplex: true,
            tls: crate::TlsServerOptions {
                cert_pem: Some(cert),
                key_pem: Some(key),
                ..Default::default()
            },
            alpn: vec!["rhtp".into()],
        };
        let mut listener = listen(&server_cfg).await.unwrap();
        let addr = listener.local_addr;

        let srv = tokio::spawn(async move {
            let inc = listener.accept().await.unwrap();
            let mut s = inc.mux.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 8];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(&buf).await.unwrap();
        });

        let client = ClientTransportConfig {
            protocol: Protocol::Quic,
            server_addr: addr,
            multiplex: true,
            tls: crate::TlsClientOptions {
                insecure_skip_verify: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let mux = crate::connect(&client).await.unwrap();
        let mut s = mux.open_stream().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        s.write_all(b"quic-ok!").await.unwrap();
        let mut back = [0u8; 8];
        s.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"quic-ok!");
        srv.await.unwrap();
    }
}
