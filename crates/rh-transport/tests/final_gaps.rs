//! final-gaps 定向补修回归:vendor yamux lazy 流 close-before-first-write。
//!
//! 缺陷(主代理独立复现 /tmp/rh-main-independent-review/src/main.rs
//! close_before_write,修前 exit=101):`open_stream` 后不 write 立即
//! shutdown,vendor `poll_close`/`poll_close_half` 只消费 `Flag::Ack`
//! 不消费 `Flag::Syn`,FIN 无 SYN 上线,对端从未见到开流,accept 超时。
//!
//! 修复语义:未随数据帧发出的 SYN 必须随 FIN 合帧发出(Data len0 +
//! SYN|FIN 是标准 yamux wire 组合);split/unsplit 两个 API 一致;
//! 空 write 不得吞 SYN(空数据帧携带 SYN 或留给 close);重复 close
//! 幂等(单 FIN);半关闭后对端仍可写回应。
//! 全部用例整体 deadline(防挂死),修前红/修后绿(标注"锁"的除外)。

use rh_transport::{mux, BoxedIo};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const DEADLINE: Duration = Duration::from_secs(5);

/// 公共夹具:duplex 上的真实 client/server yamux 对。
fn peer_pair() -> (
    std::sync::Arc<rh_transport::Multiplexer>,
    std::sync::Arc<rh_transport::Multiplexer>,
) {
    let (a, b) = tokio::io::duplex(4096);
    let client = mux::wrap_stream_as_client(Box::new(a) as BoxedIo, true, 100);
    let server = mux::wrap_stream_as_server(Box::new(b) as BoxedIo, true, 100);
    (client, server)
}

/// (红→绿)unsplit API:主代理复现原样——open → 立即 shutdown →
/// 对端 accept 必须就绪、读到 EOF、写回应、shutdown,客户端收到回应。
#[tokio::test]
async fn close_before_write_real_peer_unsplit() {
    tokio::time::timeout(DEADLINE, async {
        let (client, server) = peer_pair();
        let mut c = client.open_stream().await.unwrap();
        c.shutdown().await.unwrap();

        let mut s = server
            .accept()
            .await
            .expect("closing a lazy outbound stream must still announce SYN to the peer");
        let mut probe = [0u8; 1];
        assert_eq!(
            s.read(&mut probe).await.unwrap(),
            0,
            "peer must see immediate EOF (half-close)"
        );
        s.write_all(b"response").await.unwrap();
        s.shutdown().await.unwrap();

        let mut reply = Vec::new();
        c.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, b"response");
    })
    .await
    .expect("整个用例 deadline");
}

/// (红→绿)split API:偷取 halves 后写半直接 close(无任何 write_chunk),
/// 对端 accept + EOF + 回应经读半完整送达。
#[tokio::test]
async fn close_before_write_real_peer_split() {
    tokio::time::timeout(DEADLINE, async {
        let (client, server) = peer_pair();
        let mut c = client.open_stream().await.unwrap();
        let (mut c_read, mut c_write) = mux::steal_mux_halves(&mut c).unwrap();
        c_write.close().await.unwrap();

        let mut s = server
            .accept()
            .await
            .expect("split close-before-write must still announce SYN");
        let mut probe = [0u8; 1];
        assert_eq!(s.read(&mut probe).await.unwrap(), 0, "peer must see EOF");
        s.write_all(b"response").await.unwrap();
        s.shutdown().await.unwrap();

        let mut got = Vec::new();
        while let Some(b) = c_read.read_chunk().await.unwrap() {
            got.extend_from_slice(&b);
        }
        assert_eq!(got, b"response");
    })
    .await
    .expect("整个用例 deadline");
}

/// (锁)空 write(空 chunk)不得吞掉 SYN:空 chunk 作为 Data len0 帧携带
/// SYN 上线(vendor `poll_write_chunk` 对 Some(空) 走完整帧路径),随后
/// close 只补 FIN;对端必须恰好见到一次开流并读到 EOF;回应送达后连接
/// 仍可继续开第二条流(证明无重复 SYN / 协议错误)。
#[tokio::test]
async fn empty_chunk_then_close_announces_stream_once() {
    tokio::time::timeout(DEADLINE, async {
        let (client, server) = peer_pair();
        let mut c = client.open_stream().await.unwrap();
        let (mut c_read, mut c_write) = mux::steal_mux_halves(&mut c).unwrap();
        c_write.write_chunk(bytes::Bytes::new()).await.unwrap();
        c_write.close().await.unwrap();

        let mut s = server
            .accept()
            .await
            .expect("empty write must not consume the pending SYN");
        let mut probe = [0u8; 1];
        assert_eq!(s.read(&mut probe).await.unwrap(), 0);
        s.write_all(b"r1").await.unwrap();
        s.shutdown().await.unwrap();
        let mut got = Vec::new();
        while let Some(b) = c_read.read_chunk().await.unwrap() {
            got.extend_from_slice(&b);
        }
        assert_eq!(got, b"r1");

        // 连接健康:第二条流双向正常(重复 SYN 会被对端按协议错误断连)
        let mut c2 = client.open_stream().await.unwrap();
        c2.write_all(b"second").await.unwrap();
        c2.shutdown().await.unwrap();
        let mut s2 = server.accept().await.unwrap();
        let mut got2 = Vec::new();
        s2.read_to_end(&mut got2).await.unwrap();
        assert_eq!(got2, b"second");
    })
    .await
    .expect("整个用例 deadline");
}

/// (锁)重复 close 幂等 + 半关闭回复:先写数据再 close 两次,对端读到
/// 完整数据 + 单次 EOF,回应可写回,连接随后仍可用。
#[tokio::test]
async fn repeated_close_and_half_close_reply() {
    tokio::time::timeout(DEADLINE, async {
        let (client, server) = peer_pair();
        let mut c = client.open_stream().await.unwrap();
        c.write_all(b"hi").await.unwrap();
        c.shutdown().await.unwrap();
        // 第二次 close 必须幂等(不得再排队第二个 FIN)
        c.shutdown().await.unwrap();

        let mut s = server.accept().await.unwrap();
        let mut got = Vec::new();
        s.read_to_end(&mut got).await.unwrap();
        assert_eq!(got, b"hi");
        s.write_all(b"ok").await.unwrap();
        s.shutdown().await.unwrap();

        let mut reply = Vec::new();
        c.read_to_end(&mut reply).await.unwrap();
        assert_eq!(reply, b"ok");

        let mut c2 = client.open_stream().await.unwrap();
        c2.write_all(b"again").await.unwrap();
        c2.shutdown().await.unwrap();
        let mut s2 = server.accept().await.unwrap();
        let mut got2 = Vec::new();
        s2.read_to_end(&mut got2).await.unwrap();
        assert_eq!(got2, b"again");
    })
    .await
    .expect("整个用例 deadline");
}
