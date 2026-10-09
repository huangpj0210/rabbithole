//! WebSocket 载体:在任意字节流上做 HTTP Upgrade 握手,再把 WS 二进制消息
//! 适配为 AsyncRead/AsyncWrite 字节流(消息边界视作连续字节)。

use crate::{base64_encode, BoxedIo};
use futures::{Sink, Stream};
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio_tungstenite::tungstenite::protocol::{Message, Role};

pub const WS_PATH: &str = "/rhtp";

/// 服务端:完成 WS 升级握手,返回字节流。
pub async fn upgrade_server(mut io: BoxedIo, expect_path: &str) -> io::Result<BoxedIo> {
    // 读到请求头结尾
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), io.read(&mut chunk))
            .await
            .map_err(|_| io::Error::other("ws handshake timeout"))??;
        if n == 0 {
            return Err(io::Error::other("ws handshake: client closed"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err(io::Error::other("ws handshake request too large"));
        }
    }
    let req = String::from_utf8_lossy(&buf).into_owned();
    let first_line = req.lines().next().unwrap_or_default().to_string();
    let mut path_ok = false;
    if let Some(path) = first_line.split_whitespace().nth(1) {
        let p = path.split('?').next().unwrap_or("");
        path_ok = p == expect_path || (expect_path == "/" && p == "/");
    }
    if !path_ok {
        let _ = io
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
            .await;
        return Err(io::Error::other("ws: unexpected path"));
    }
    // 生成 Sec-WebSocket-Accept
    let key = req
        .lines()
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.trim().eq_ignore_ascii_case("sec-websocket-key") {
                Some(v.trim().to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| io::Error::other("ws: missing Sec-WebSocket-Key"))?;
    let accept = ws_accept(&key);
    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    io.write_all(resp.as_bytes()).await?;
    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(io, Role::Server, None).await;
    Ok(Box::new(WsIo::new(ws)))
}

/// 客户端:发起 WS 升级握手,返回字节流。
pub async fn client_handshake(mut io: BoxedIo, host: &str, path: &str) -> io::Result<BoxedIo> {
    use rand::RngCore;
    let mut key_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut key_bytes);
    let key = base64_encode(&key_bytes);
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n"
    );
    io.write_all(req.as_bytes()).await?;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), io.read(&mut chunk))
            .await
            .map_err(|_| io::Error::other("ws handshake timeout"))??;
        if n == 0 {
            return Err(io::Error::other("ws handshake: server closed"));
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if buf.len() > 16 * 1024 {
            return Err(io::Error::other("ws handshake response too large"));
        }
    }
    let resp = String::from_utf8_lossy(&buf).into_owned();
    let status_ok = resp
        .lines()
        .next()
        .map(|l| l.contains("101"))
        .unwrap_or(false);
    if !status_ok {
        return Err(io::Error::other(format!(
            "ws upgrade failed: {}",
            resp.lines().next().unwrap_or("")
        )));
    }
    // 校验 accept
    let want = ws_accept(&key);
    let got = resp.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        if k.trim().eq_ignore_ascii_case("sec-websocket-accept") {
            Some(v.trim().to_string())
        } else {
            None
        }
    });
    if got.as_deref() != Some(want.as_str()) {
        return Err(io::Error::other("ws: Sec-WebSocket-Accept mismatch"));
    }
    let ws = tokio_tungstenite::WebSocketStream::from_raw_socket(io, Role::Client, None).await;
    Ok(Box::new(WsIo::new(ws)))
}

fn ws_accept(key: &str) -> String {
    use sha2::Digest;
    const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut h = sha2::Sha256::new();
    h.update(key.as_bytes());
    h.update(GUID.as_bytes());
    base64_encode(&h.finalize())
}

// WS 消息流 → 字节流适配。
pin_project_lite::pin_project! {
struct WsIo {
    #[pin]
    ws: tokio_tungstenite::WebSocketStream<BoxedIo>,
    rd_buf: Vec<u8>,
    rd_pos: usize,
    eof: bool,
}
}

impl WsIo {
    fn new(ws: tokio_tungstenite::WebSocketStream<BoxedIo>) -> Self {
        Self {
            ws,
            rd_buf: Vec::new(),
            rd_pos: 0,
            eof: false,
        }
    }
}

impl AsyncRead for WsIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let mut this = self.project();
        loop {
            if *this.rd_pos < this.rd_buf.len() {
                let avail = &this.rd_buf[*this.rd_pos..];
                let n = avail.len().min(buf.remaining());
                buf.put_slice(&avail[..n]);
                *this.rd_pos += n;
                if *this.rd_pos == this.rd_buf.len() {
                    this.rd_buf.clear();
                    *this.rd_pos = 0;
                }
                return Poll::Ready(Ok(()));
            }
            if *this.eof {
                return Poll::Ready(Ok(()));
            }
            match futures::ready!(this.ws.as_mut().poll_next(cx)) {
                None => {
                    *this.eof = true;
                    return Poll::Ready(Ok(()));
                }
                Some(Ok(Message::Binary(data))) => {
                    *this.rd_buf = data.to_vec();
                    *this.rd_pos = 0;
                }
                Some(Ok(Message::Close(_))) => {
                    *this.eof = true;
                    return Poll::Ready(Ok(()));
                }
                Some(Ok(_)) => continue, // 文本/ping/pong 等忽略
                Some(Err(e)) => return Poll::Ready(Err(io::Error::other(format!("ws: {e}")))),
            }
        }
    }
}

impl AsyncWrite for WsIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let mut this = self.project();
        let msg = Message::Binary(buf.to_vec().into());
        match futures::ready!(this.ws.as_mut().poll_ready(cx)) {
            Ok(()) => match this.ws.as_mut().start_send(msg) {
                Ok(()) => {
                    // 每帧立即 flush,保证 write_all 后数据上 wire
                    futures::ready!(this.ws.as_mut().poll_flush(cx))
                        .map_err(|e| io::Error::other(format!("ws flush: {e}")))?;
                    Poll::Ready(Ok(buf.len()))
                }
                Err(e) => Poll::Ready(Err(io::Error::other(format!("ws: {e}")))),
            },
            Err(e) => Poll::Ready(Err(io::Error::other(format!("ws: {e}")))),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut this = self.project();
        Poll::Ready(
            futures::ready!(this.ws.as_mut().poll_flush(cx))
                .map_err(|e| io::Error::other(format!("ws flush: {e}"))),
        )
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let mut this = self.project();
        Poll::Ready(
            futures::ready!(this.ws.as_mut().poll_close(cx))
                .map_err(|e| io::Error::other(format!("ws close: {e}"))),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ws_handshake_and_stream() {
        let (a, b) = tokio::io::duplex(8192);
        let server = tokio::spawn(async move {
            let io: BoxedIo = Box::new(b);
            let s = upgrade_server(io, WS_PATH).await.unwrap();
            s
        });
        let client_io: BoxedIo = Box::new(a);
        let mut c = client_handshake(client_io, "127.0.0.1", WS_PATH)
            .await
            .unwrap();
        let mut s = server.await.unwrap();
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        c.write_all(b"hello-ws").await.unwrap();
        let mut buf = [0u8; 8];
        s.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello-ws");
        s.write_all(b"ok").await.unwrap();
        let mut back = [0u8; 2];
        c.read_exact(&mut back).await.unwrap();
        assert_eq!(&back, b"ok");
    }

    #[tokio::test]
    async fn ws_wrong_path_rejected() {
        let (a, b) = tokio::io::duplex(1024);
        tokio::spawn(async move {
            let io: BoxedIo = Box::new(b);
            let _ = upgrade_server(io, WS_PATH).await;
        });
        let mut raw = a;
        raw.write_all(b"GET /other HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: abc\r\nSec-WebSocket-Version: 13\r\n\r\n")
            .await
            .unwrap();
        let mut resp = Vec::new();
        let mut ch = [0u8; 128];
        let n = raw.read(&mut ch).await.unwrap();
        resp.extend_from_slice(&ch[..n]);
        let text = String::from_utf8_lossy(&resp);
        assert!(text.contains("404"));
    }
}
