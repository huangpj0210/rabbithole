//! snappy 流压缩(代理级 use_compression)。
//!
//! 实现:pump 任务 + duplex 管道 —— 用户侧拿到一条普通字节流,
//! 两个后台任务分别负责「压缩写」与「解压读」,压缩逻辑全部是
//! async fn 循环(read_exact/write_all),不手写任何 poll 状态机。
//!
//! 帧格式:`varint(压缩块长) + snappy raw 块`,单块未压缩 ≤64KiB。
//! 流损坏/截断时 pump 退出,上层表现为连接断开。

use crate::BoxedIo;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const MAX_UNCOMPRESSED_CHUNK: usize = 64 * 1024;
const MAX_DECOMPRESSED_CHUNK: usize = 4 * 1024 * 1024;

/// 把 `inner` 包一层端到端 snappy 压缩,返回面向调用方的普通字节流。
/// 丢弃返回值时 pump 任务随之结束并释放 inner。
pub async fn compress_stream(inner: BoxedIo) -> BoxedIo {
    let (user_side, pump_side) = tokio::io::duplex(64 * 1024);
    let (pump_r, pump_w) = tokio::io::split(pump_side);
    let (inner_r, inner_w) = tokio::io::split(inner);
    tokio::spawn(compress_pump(pump_r, inner_w));
    tokio::spawn(decompress_pump(inner_r, pump_w));
    Box::new(user_side)
}

fn push_varint(out: &mut Vec<u8>, mut v: usize) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

async fn compress_pump<R, W>(mut src: R, mut dst: W)
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    // 任何退出路径都显式关闭写方向(WriteHalf drop 不会关闭 duplex)
    let mut enc = snap::raw::Encoder::new();
    let mut buf = vec![0u8; MAX_UNCOMPRESSED_CHUNK];
    loop {
        match src.read(&mut buf).await {
            Ok(0) => break,
            Ok(n) => {
                let mut comp = vec![0u8; snap::raw::max_compress_len(n)];
                let clen = match enc.compress(&buf[..n], &mut comp) {
                    Ok(c) => c,
                    Err(_) => break,
                };
                comp.truncate(clen);
                let mut wire = Vec::with_capacity(comp.len() + 8);
                push_varint(&mut wire, clen);
                wire.append(&mut comp);
                if dst.write_all(&wire).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    let _ = dst.shutdown().await;
}

async fn decompress_pump<R, W>(mut src: R, mut dst: W)
where
    R: AsyncReadExt + Unpin,
    W: AsyncWriteExt + Unpin,
{
    loop {
        // varint 长度(≤5 字节)
        let mut len: usize = 0;
        let mut shift = 0;
        let mut done = false;
        for _ in 0..5 {
            let mut b = [0u8; 1];
            match src.read_exact(&mut b).await {
                Ok(_) => {
                    len |= ((b[0] & 0x7f) as usize) << shift;
                    if b[0] & 0x80 == 0 {
                        done = true;
                        break;
                    }
                    shift += 7;
                }
                Err(_) => break, // EOF:正常结束;半截 = 上层连接错误
            }
        }
        if !done {
            break;
        }
        if len == 0 || len > snap::raw::max_compress_len(MAX_UNCOMPRESSED_CHUNK) {
            break;
        }
        let mut comp = vec![0u8; len];
        if src.read_exact(&mut comp).await.is_err() {
            break;
        }
        let expected = match snap::raw::decompress_len(&comp) {
            Ok(e) => e,
            Err(_) => break,
        };
        if expected > MAX_DECOMPRESSED_CHUNK {
            break;
        }
        match snap::raw::Decoder::new().decompress_vec(&comp) {
            Ok(out) => {
                if dst.write_all(&out).await.is_err() {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    // 显式关闭:上层 read 将收到 EOF 而非永久 Pending
    let _ = dst.shutdown().await;
}

/// 解压方向的单帧解码(测试与诊断用)。
pub fn decode_frame(comp: &[u8]) -> io::Result<Vec<u8>> {
    let expected =
        snap::raw::decompress_len(comp).map_err(|e| io::Error::other(format!("compress: {e}")))?;
    if expected > MAX_DECOMPRESSED_CHUNK {
        return Err(io::Error::other("compress: chunk too large"));
    }
    snap::raw::Decoder::new()
        .decompress_vec(comp)
        .map_err(|e| io::Error::other(format!("compress: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn roundtrip_small() {
        let (a, b) = tokio::io::duplex(4096);
        let compressed = compress_stream(Box::new(b)).await;
        let mut w = compressed;
        let mut r_side = a;
        w.write_all(b"hello compress!").await.unwrap();
        w.flush().await.unwrap();
        // 直接从对端读的是压缩字节;先简单验证非空且与原文不同
        let mut wire = vec![0u8; 64];
        let n = tokio::time::timeout(std::time::Duration::from_secs(2), r_side.read(&mut wire))
            .await
            .unwrap()
            .unwrap();
        assert!(n > 0);
        assert_ne!(&wire[..n], b"hello compress!");
    }

    /// 端到端:两条 compress_stream 互连,验证数据完整性。
    #[tokio::test]
    async fn end_to_end_roundtrip_bulk() {
        // 模拟网络管道
        let (net_a, net_b) = tokio::io::duplex(64 * 1024);
        let client_stream = compress_stream(Box::new(net_a)).await;
        let server_stream = compress_stream(Box::new(net_b)).await;

        let mut data = vec![0xABu8; 3_000_000];
        let mut pseudo = Vec::with_capacity(200_000);
        let mut x: u64 = 0x2545F4914F6CDD1D;
        for _ in 0..200_000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            pseudo.push((x >> 24) as u8);
        }
        data.extend_from_slice(&pseudo);

        let writer = tokio::spawn(async move {
            let mut w = client_stream;
            w.write_all(&data).await.unwrap();
            w.shutdown().await.unwrap();
        });
        let reader = tokio::spawn(async move {
            let mut r = server_stream;
            let mut got = Vec::with_capacity(3_200_000);
            let mut buf = [0u8; 32 * 1024];
            loop {
                let n = tokio::time::timeout(std::time::Duration::from_secs(30), r.read(&mut buf))
                    .await
                    .expect("read timeout")
                    .unwrap();
                if n == 0 {
                    break;
                }
                got.extend_from_slice(&buf[..n]);
            }
            got
        });
        writer.await.unwrap();
        let got = reader.await.unwrap();
        assert_eq!(got.len(), 3_200_000);
        assert!(got[..3_000_000].iter().all(|&v| v == 0xAB));
        let mut x2: u64 = 0x2545F4914F6CDD1D;
        for (i, g) in got[3_000_000..].iter().enumerate() {
            x2 ^= x2 << 13;
            x2 ^= x2 >> 7;
            x2 ^= x2 << 17;
            assert_eq!(*g, (x2 >> 24) as u8, "pseudo mismatch at {}", i + 3_000_000);
        }
    }

    #[tokio::test]
    async fn corrupt_stream_closes() {
        let (mut a, b) = tokio::io::duplex(1024);
        // 对端发送非法长度后关闭
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            let _ = a.write_all(&[0xff, 0xff, 0xff, 0xff, 0x7f]).await;
        });
        let mut r = compress_stream(Box::new(b)).await;
        let mut out = vec![0u8; 16];
        // 解压泵退出 → EOF(可能读到 0 字节或错误)
        let _ = r.read(&mut out).await;
        drop(tokio::io::duplex(1));
    }
}
