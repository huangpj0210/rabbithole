//! yamux 多路复用驱动:yamux 0.14 是 poll 风格 + futures IO,这里封装为
//! async open/accept(返回 tokio IO)+ 独立驱动任务。

use crate::{BoxedIo, MultiplexerInner};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::Poll;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{Compat, FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};

type MuxStream = Compat<yamux::Stream>;
type MuxConn = yamux::Connection<Compat<BoxedIo>>;

pub struct YamuxMultiplexer {
    open_tx: mpsc::Sender<oneshot::Sender<io::Result<MuxStream>>>,
    accept_rx: tokio::sync::Mutex<mpsc::Receiver<MuxStream>>,
    dead: Arc<AtomicBool>,
}

impl YamuxMultiplexer {
    /// 在给定字节流上创建 yamux;mode:客户端(开流)/服务端(收流)。
    /// keepalive 30s:同时用于维持 KCP 会话活跃与死连接探测。
    pub fn new(io: BoxedIo, mode: yamux::Mode) -> Self {
        let mut cfg = yamux::Config::default();
        cfg.set_max_num_streams(256);
        let conn: MuxConn = yamux::Connection::new(io.compat(), cfg, mode);
        let (open_tx, open_rx) = mpsc::channel::<oneshot::Sender<io::Result<MuxStream>>>(64);
        let (accept_tx, accept_rx) = mpsc::channel::<MuxStream>(256);
        let dead = Arc::new(AtomicBool::new(false));
        tokio::spawn(driver(conn, open_rx, accept_tx, dead.clone()));
        Self {
            open_tx,
            accept_rx: tokio::sync::Mutex::new(accept_rx),
            dead,
        }
    }

    pub async fn open_stream(&self) -> io::Result<BoxedIo> {
        let (tx, rx) = oneshot::channel();
        self.open_tx
            .send(tx)
            .await
            .map_err(|_| other("mux driver gone"))?;
        let stream = rx
            .await
            .map_err(|_| other("mux driver dropped request"))??;
        Ok(Box::new(stream))
    }

    pub async fn accept_stream(&self) -> io::Result<BoxedIo> {
        let mut guard = self.accept_rx.lock().await;
        match guard.recv().await {
            Some(s) => Ok(Box::new(s)),
            None => Err(other("mux connection closed")),
        }
    }

    pub fn is_dead(&self) -> bool {
        self.dead.load(Ordering::Relaxed)
    }
}

fn other(msg: &str) -> io::Error {
    io::Error::other(msg.to_string())
}

enum Ev {
    Req(oneshot::Sender<io::Result<MuxStream>>),
    Inbound(MuxStream),
    Done,
}

async fn driver(
    mut conn: MuxConn,
    mut open_rx: mpsc::Receiver<oneshot::Sender<io::Result<MuxStream>>>,
    accept_tx: mpsc::Sender<MuxStream>,
    dead: Arc<AtomicBool>,
) {
    let mut pending_open: Option<oneshot::Sender<io::Result<MuxStream>>> = None;
    let mut buffered_inbound: Option<MuxStream> = None;
    loop {
        // 有待交付的入站流:等接收方取走(背压)。
        if let Some(stream) = buffered_inbound.take() {
            if accept_tx.send(stream).await.is_err() {
                break;
            }
        }
        let ev = std::future::poll_fn(|cx| {
            if pending_open.is_none() {
                if let Poll::Ready(Some(req)) = open_rx.poll_recv(cx) {
                    return Poll::Ready(Ev::Req(req));
                }
            }
            if let Some(req) = pending_open.take() {
                match conn.poll_new_outbound(cx) {
                    Poll::Ready(Ok(s)) => {
                        let _ = req.send(Ok(s.compat()));
                    }
                    Poll::Ready(Err(e)) => {
                        let _ = req.send(Err(io::Error::other(format!("yamux: {e}"))));
                        dead.store(true, Ordering::Relaxed);
                        return Poll::Ready(Ev::Done);
                    }
                    Poll::Pending => {
                        pending_open = Some(req);
                    }
                }
            }
            match conn.poll_next_inbound(cx) {
                Poll::Ready(Some(Ok(s))) => Poll::Ready(Ev::Inbound(s.compat())),
                Poll::Ready(Some(Err(e))) => {
                    tracing::debug!(error = %e, "yamux inbound error");
                    dead.store(true, Ordering::Relaxed);
                    Poll::Ready(Ev::Done)
                }
                Poll::Ready(None) => {
                    dead.store(true, Ordering::Relaxed);
                    Poll::Ready(Ev::Done)
                }
                Poll::Pending => Poll::Pending,
            }
        })
        .await;
        match ev {
            Ev::Req(req) => {
                pending_open = Some(req);
            }
            Ev::Inbound(s) => {
                buffered_inbound = Some(s);
            }
            Ev::Done => break,
        }
    }
    dead.store(true, Ordering::Relaxed);
    if let Some(req) = pending_open.take() {
        let _ = req.send(Err(other("mux connection closed")));
    }
    while let Ok(req) = open_rx.try_recv() {
        let _ = req.send(Err(other("mux connection closed")));
    }
}

/// 服务端:把已建立的字节流包装为 Multiplexer(multiplex=true → yamux 服务端)。
pub fn wrap_stream_as_server(io: BoxedIo, multiplex: bool) -> Arc<crate::Multiplexer> {
    if multiplex {
        Arc::new(crate::Multiplexer {
            inner: MultiplexerInner::Yamux(YamuxMultiplexer::new(io, yamux::Mode::Server)),
        })
    } else {
        Arc::new(crate::Multiplexer {
            inner: MultiplexerInner::AcceptSingle {
                stream: tokio::sync::Mutex::new(Some(io)),
            },
        })
    }
}

/// 客户端:同上(yamux 客户端)。
pub fn wrap_stream_as_client(io: BoxedIo, multiplex: bool) -> Arc<crate::Multiplexer> {
    if multiplex {
        Arc::new(crate::Multiplexer {
            inner: MultiplexerInner::Yamux(YamuxMultiplexer::new(io, yamux::Mode::Client)),
        })
    } else {
        Arc::new(crate::Multiplexer {
            inner: MultiplexerInner::AcceptSingle {
                stream: tokio::sync::Mutex::new(Some(io)),
            },
        })
    }
}

/// tcpmux 服务端终结:yamux 服务端接受多路流(供 vhost/tcpmux 使用)。
pub fn tcpmux_server_mux(io: crate::BoxedIo) -> Arc<crate::Multiplexer> {
    wrap_stream_as_server(io, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn yamux_open_accept_echo() {
        let (a, b) = tokio::io::duplex(4096);
        let server = wrap_stream_as_server(Box::new(b), true);
        let client = wrap_stream_as_client(Box::new(a), true);

        let srv = server.clone();
        tokio::spawn(async move {
            let mut s = srv.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut buf = [0u8; 16];
            let n = s.read(&mut buf).await.unwrap();
            s.write_all(&buf[..n]).await.unwrap();
        });

        let mut c = client.open_stream().await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        c.write_all(b"ping").await.unwrap();
        let mut out = [0u8; 4];
        c.read_exact(&mut out).await.unwrap();
        assert_eq!(&out, b"ping");
    }

    #[tokio::test]
    async fn yamux_many_streams() {
        let (a, b) = tokio::io::duplex(8192);
        let server = wrap_stream_as_server(Box::new(b), true);
        let client = wrap_stream_as_client(Box::new(a), true);
        let srv = server.clone();
        let echo = tokio::spawn(async move {
            for _ in 0..8 {
                let mut s = match srv.accept().await {
                    Ok(s) => s,
                    Err(_) => return,
                };
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 64];
                    if let Ok(n) = s.read(&mut buf).await {
                        let _ = s.write_all(&buf[..n]).await;
                    }
                });
            }
        });
        let mut handles = Vec::new();
        for i in 0..8u8 {
            let cli = client.clone();
            handles.push(tokio::spawn(async move {
                let mut s = cli.open_stream().await.unwrap();
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let payload = vec![i; 32];
                s.write_all(&payload).await.unwrap();
                let mut out = vec![0u8; 32];
                s.read_exact(&mut out).await.unwrap();
                assert_eq!(out, payload);
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
        echo.abort();
    }

    // tcpmux 标准模式全链路:用户侧 yamux 客户端在一条通道上并发多开流;
    // 服务端(= tcpmux_server_mux)accept 每流 → 拨号本地 TCP 目标 + 裸双向搬运
    // (与 vhost.rs 的 accept 循环、rh-client handle_start_forward 的每流转发同构)。
    // 校验:并发流隔离、大负载完整、半关闭(写 EOF → 目标回 EOF)。
    #[tokio::test]
    async fn tcpmux_stream_path_over_real_target() {
        let echo = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let echo_addr = echo.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((c, _)) = echo.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let (mut r, mut w) = c.into_split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });

        let (a, b) = tokio::io::duplex(64 * 1024);
        let server = tcpmux_server_mux(Box::new(b));
        let client = wrap_stream_as_client(Box::new(a), true);

        let srv = server.clone();
        tokio::spawn(async move {
            loop {
                let Ok(mut stream) = srv.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    if let Ok(mut local) = tokio::net::TcpStream::connect(echo_addr).await {
                        let _ = tokio::io::copy_bidirectional(&mut stream, &mut local).await;
                    }
                });
            }
        });

        const N: usize = 8;
        let mut handles = Vec::new();
        for i in 0..N {
            let cli = client.clone();
            handles.push(tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut s = cli.open_stream().await.unwrap();
                let payload = vec![i as u8; 4096];
                s.write_all(&payload).await.unwrap();
                s.shutdown().await.unwrap();
                let mut out = Vec::new();
                s.read_to_end(&mut out).await.unwrap();
                assert_eq!(out.len(), payload.len());
                assert!(out.iter().all(|&b| b == i as u8));
            }));
        }
        for h in handles {
            h.await.unwrap();
        }
    }
}
