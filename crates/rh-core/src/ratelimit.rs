//! 异步令牌桶限速(带宽限制 bandwidthLimit 的实现)。
//!
//! 按字节补充令牌;`acquire(n)` 等待凑足 n 个字节额度。
//! 读写循环每次搬运前 acquire(至多 chunk),从而平滑限制吞吐。

use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Debug)]
pub struct TokenBucket {
    state: Mutex<State>,
    capacity: f64,
    rate_per_sec: f64,
}

#[derive(Debug)]
struct State {
    tokens: f64,
    last: Instant,
}

impl TokenBucket {
    /// rate:字节/秒。容量 = 1 秒的量(允许小突发)。
    pub fn new(bytes_per_sec: f64) -> Self {
        Self {
            state: Mutex::new(State {
                tokens: bytes_per_sec,
                last: Instant::now(),
            }),
            capacity: bytes_per_sec.max(1.0),
            rate_per_sec: bytes_per_sec.max(0.001),
        }
    }

    /// 非阻塞地取 up to n 字节额度,返回实际可用量(0..=n)。
    pub async fn take(&self, n: usize) -> usize {
        let mut s = self.state.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(s.last).as_secs_f64();
        s.tokens = (s.tokens + elapsed * self.rate_per_sec).min(self.capacity);
        s.last = now;
        let avail = s.tokens.floor() as usize;
        let granted = avail.min(n);
        s.tokens -= granted as f64;
        granted
    }

    /// 等待直到凑足 n 字节额度(n 超过容量时按容量分批)。
    pub async fn acquire(&self, n: usize) {
        let mut remain = n;
        while remain > 0 {
            let got = self.take(remain).await;
            remain -= got;
            if remain > 0 {
                let wait = (remain as f64 / self.rate_per_sec).min(1.0);
                tokio::time::sleep(Duration::from_secs_f64(wait.max(0.001))).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rate_limit_basic() {
        // 1MB/s,容量 1MB
        let bucket = TokenBucket::new(1024.0 * 1024.0);
        // 首秒容量内一次取满
        assert_eq!(bucket.take(1024 * 1024).await, 1024 * 1024);
        // 已耗尽:两次调用间的微秒级流逝只可能补回极少量
        assert!(bucket.take(1024).await < 512);
        tokio::time::sleep(Duration::from_millis(30)).await;
        let got = bucket.take(1024 * 1024).await;
        // ~30ms 补充 ~30KB
        assert!(got > 10_000 && got < 60_000, "got={got}");
    }
}
