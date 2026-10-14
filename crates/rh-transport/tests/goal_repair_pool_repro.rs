//! A4 公共入口长期锁(独立测试二进制,方案 §6a 放置规则)。
//!
//! 修前证据:benchmark 侧独立 main 的 9 次失败记录(repro-results.txt,
//! cap ×3:requested_cap=65536 actual_read=262144)——修前 `pool::put`
//! 收纳 256KiB 超限缓冲,`read_chunk_from(..., 64KiB)` 经公共入口取到
//! 超限容量。
//!
//! B3 合入后 `pool::put` 拒收 cap>CLASS_MAX(72KiB)的缓冲,超限播种成为
//! no-op——本测试的原始复现形态不再可构造(独立审查三轮指出旧形态已
//! 恒真)。保留并改注为**公共入口的合成红线锁**:经公共 API 播种超限
//! 缓冲(256KiB×256)后,`read_chunk_from` 的每块长度仍不得超过 64KiB
//! (红线 2 端到端口径;Bytes 冻结后容量不可观,长度即公共入口可观测的
//! 上限量)。分解锁:B3 拒收 = vendor pool 测试,A4 读入收口 =
//! mux.rs read_chunk_from_cap_lock(直注 256KiB 缓冲的私有缝)。
//! 载荷逐字节完整(192KiB 全部经池相缓冲读出)。

use bytes::BytesMut;

#[tokio::test]
async fn read_chunk_from_cap_repro() {
    for _ in 0..256 {
        yamux::pool::put(BytesMut::with_capacity(256 * 1024));
    }
    // 载荷 ≥128KiB:跨过 C4 修复后的初始相本地缓冲,进入池相取缓冲
    let input = vec![7u8; 192 * 1024];
    let mut reader = &input[..];
    let mut total = 0usize;
    loop {
        let chunk = rh_transport::mux::read_chunk_from(&mut reader, 64 * 1024)
            .await
            .unwrap()
            .expect("input not exhausted");
        assert!(
            chunk.len() <= 64 * 1024,
            "A4:读块 {} 超过 64KiB 上限(红线 2;修前经公共入口可读出 262144)",
            chunk.len()
        );
        assert!(chunk.iter().all(|&b| b == 7));
        total += chunk.len();
        if total == input.len() {
            break;
        }
    }
    assert_eq!(total, input.len());
}
