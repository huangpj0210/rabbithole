//! g1_splice_bench —— G1 固定工作量对照 example(fixed-ab-v1 契约,
//! 见 benchmark/runs/g1-tcp-20261014/example-contract.md)。
//!
//! 真实驱动**生产泵** `rh_transport::splice::splice_relay`(data.rs 实际
//! 装配的同一条零拷贝路径),本地 socketpair 拓扑:source → relay → sink。
//! baseline/candidate 两臂共用本文件、同一钩子(DirHook 计量)与同一
//! release 默认 flags——唯一差异是底层泵实现,因此本文件只依赖两臂
//! 均存在的公共 API(splice_relay/SpliceHooks/DirHook 四字段),不引用
//! 候选臂新增符号。
//!
//! 计数来源如实声明(fixed-ab 只消费 stdout 的 schema/gbps,计数字段
//! 为归因原始数据):
//! * fill_calls/fill_bytes:活动方向 DirHook(meter_on_fill=true,回调
//!   点位=每次 fill 块完成)——两臂同钩子,真实可比。
//! * drain_bytes:sink 应用层实收(送达确认,可与 fill_bytes 交叉校验);
//!   drain_calls:单 DirHook 单计量点位下不可观测,恒 0(不冒充)。
//! * eagain_*/pending_waits_*/coop_yields:泵内部计数,仅候选臂经
//!   RH_SPLICE_STATS 输出到 stderr(本 example 启动时置位;基线臂不
//!   识别该 env,无副作用);stdout 中恒 0。
//!
//! 只做本地归因对照,不冒充三机 G1 判定(契约 §1.4/§4)。

#[cfg(target_os = "linux")]
fn main() {
    // 候选臂泵在 RH_SPLICE_STATS 开启时把每泵 v2 计数以单行 JSON 输出
    // 到 stderr(relay 初始化时读取一次 env,此处置位即生效;基线臂不
    // 识别,无副作用)
    if std::env::var("RH_SPLICE_STATS").is_err() {
        std::env::set_var("RH_SPLICE_STATS", "1");
    }
    eprintln!(
        "[g1_splice_bench] counters note: fill_* from DirHook(fill口径,两臂可比); \
         drain_bytes=sink实收; drain_calls/eagain_*/pending_*/coop_yields 在 stdout 恒 0 \
         (泵内部计数仅候选臂经 stderr RH_SPLICE_STATS 输出)"
    );
    let code = linux::run();
    std::process::exit(code);
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("g1_splice_bench 仅支持 Linux(splice(2) 生产路径)");
    std::process::exit(2);
}

#[cfg(target_os = "linux")]
mod linux {
    use rh_transport::splice::{splice_relay, DirHook, SpliceHooks};
    use std::io;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    const MIB: u64 = 1024 * 1024;
    /// 契约 §3:固定 8192 次 × 64B 小包往返(低负载)。
    const SMALL_COUNT: usize = 8192;
    const SMALL_SIZE: usize = 64;
    const WRITE_CHUNK: usize = 256 * 1024;

    fn fin(v: f64, what: &str) -> Result<f64, String> {
        if v.is_finite() {
            Ok(v)
        } else {
            Err(format!("非有限数值({what}={v}),拒绝输出 NaN/Infinity"))
        }
    }

    /// /proc/self/stat 的 utime/stime(ticks)。
    fn cpu_ticks() -> Option<(u64, u64)> {
        let s = std::fs::read_to_string("/proc/self/stat").ok()?;
        let after = s.rsplit(')').next()?;
        let f: Vec<&str> = after.split_whitespace().collect();
        Some((f.get(11)?.parse().ok()?, f.get(12)?.parse().ok()?))
    }

    fn clk_tck() -> f64 {
        unsafe { libc::sysconf(libc::_SC_CLK_TCK) as f64 }.max(1.0)
    }

    async fn tcp_pair() -> io::Result<(TcpStream, TcpStream)> {
        let ln = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = ln.local_addr()?;
        let c = TcpStream::connect(addr).await?;
        let (s, _) = ln.accept().await?;
        // 低负载小包阶段对 Nagle 敏感:显式 NODELAY(生产路径同样语义),
        // 不触碰 SO_RCVBUF/SO_SNDBUF(红线)
        let _ = c.set_nodelay(true);
        let _ = s.set_nodelay(true);
        Ok((c, s))
    }

    fn meter_hook(calls: Arc<AtomicU64>, bytes: Arc<AtomicU64>) -> DirHook {
        DirHook {
            on_bytes: Some(Arc::new(move |n| {
                calls.fetch_add(1, Ordering::Relaxed);
                bytes.fetch_add(n, Ordering::Relaxed);
            })),
            meter_on_fill: true, // fill 口径计数(回调点位=fill 块完成)
            limiter: None,
        }
    }

    /// 小包低负载阶段:新 relay + b 侧 echo,8192×64B 同步往返。
    async fn small_packet_stage() -> Result<(f64, f64), String> {
        let (mut a_cli, a_srv) = tcp_pair().await.map_err(|e| e.to_string())?;
        let (mut b_cli, b_srv) = tcp_pair().await.map_err(|e| e.to_string())?;
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, SpliceHooks::default())
                .await
                .map_err(|e| e.to_string())
        });
        let echo = tokio::spawn(async move {
            let mut buf = [0u8; SMALL_SIZE];
            while b_cli.read_exact(&mut buf).await.is_ok() {
                if b_cli.write_all(&buf).await.is_err() {
                    break;
                }
            }
        });
        let mut lat_us: Vec<f64> = Vec::with_capacity(SMALL_COUNT);
        let req = vec![0x5au8; SMALL_SIZE];
        let mut rsp = vec![0u8; SMALL_SIZE];
        for _ in 0..SMALL_COUNT {
            let t0 = Instant::now();
            a_cli.write_all(&req).await.map_err(|e| e.to_string())?;
            a_cli
                .read_exact(&mut rsp)
                .await
                .map_err(|e| format!("小包响应读取失败: {e}"))?;
            if rsp[..] != req[..] {
                return Err("小包回显数据不一致".to_string());
            }
            lat_us.push(t0.elapsed().as_secs_f64() * 1e6);
        }
        // 双侧半关闭让两条泵自然结束
        let _ = a_cli.shutdown().await;
        tokio::time::timeout(Duration::from_secs(10), relay)
            .await
            .map_err(|_| "小包 relay 未在 10s 内收敛".to_string())?
            .map_err(|e| e.to_string())??;
        tokio::time::timeout(Duration::from_secs(5), echo)
            .await
            .map_err(|_| "echo 未收敛".to_string())?
            .map_err(|e| e.to_string())?;
        lat_us.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = fin(lat_us[lat_us.len() * 50 / 100], "p50_us")?;
        let p99 = fin(lat_us[(lat_us.len() - 1) * 99 / 100], "p99_us")?;
        Ok((p50, p99))
    }

    pub fn run() -> i32 {
        let mut direction: Option<String> = None;
        let mut mib: Option<u64> = None;
        let mut json = false;
        let mut args = std::env::args().skip(1);
        while let Some(a) = args.next() {
            match a.as_str() {
                "--json" => json = true,
                "--direction" => direction = args.next(),
                "--mib" => {
                    mib = args.next().and_then(|v| v.parse().ok());
                }
                other => {
                    eprintln!("未知参数: {other}");
                    return 4;
                }
            }
        }
        let direction = direction.unwrap_or_default();
        if direction != "upload" && direction != "download" {
            eprintln!("--direction 必须是 upload 或 download(得到 {direction:?})");
            return 4;
        }
        let mib = mib.unwrap_or(0);
        if mib < 128 {
            eprintln!("--mib 必须 ≥128(契约 §2;得到 {mib})");
            return 4;
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        let res = rt.block_on(bench(direction.clone(), mib));
        let doc = match res {
            Ok(doc) => doc,
            Err(msg) => {
                let d = serde_json::json!({
                    "schema": "fixed-ab-v1",
                    "direction": direction,
                    "mib": mib,
                    "error": msg,
                });
                println!("{d}");
                return 1;
            }
        };
        if json {
            println!("{doc}");
        } else {
            eprintln!("{doc}");
        }
        0
    }

    async fn bench(direction: String, mib: u64) -> Result<serde_json::Value, String> {
        let bytes = mib * MIB;
        let upload = direction == "upload";
        // 拓扑:a_cli --(up:a→b)-- b_cli;download 时活动方向为 down(b→a)
        let (a_cli, a_srv) = tcp_pair().await.map_err(|e| e.to_string())?;
        let (b_cli, b_srv) = tcp_pair().await.map_err(|e| e.to_string())?;
        let (src, sink) = if upload {
            (a_cli, b_cli)
        } else {
            (b_cli, a_cli)
        };
        let fill_calls = Arc::new(AtomicU64::new(0));
        let fill_bytes = Arc::new(AtomicU64::new(0));
        let hooks = SpliceHooks {
            up: if upload {
                Some(meter_hook(fill_calls.clone(), fill_bytes.clone()))
            } else {
                None
            },
            down: if upload {
                None
            } else {
                Some(meter_hook(fill_calls.clone(), fill_bytes.clone()))
            },
            abort: None,
            cancel: None,
        };
        let cpu0 = cpu_ticks();
        let t0 = Instant::now();
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks)
                .await
                .map_err(|e| e.to_string())
        });
        // source:固定工作量写入 + 半关闭(触发活动泵 EOF)
        let payload = vec![0xc3u8; WRITE_CHUNK];
        let writer = tokio::spawn(async move {
            let mut src = src;
            let mut left = bytes as usize;
            while left > 0 {
                let n = left.min(payload.len());
                src.write_all(&payload[..n])
                    .await
                    .map_err(|e| e.to_string())?;
                left -= n;
            }
            src.shutdown().await.map_err(|e| e.to_string())
        });
        // sink:读满固定工作量;每满 1MiB 记录 per_mib 行
        let mut per_mib: Vec<serde_json::Value> = Vec::with_capacity(mib as usize);
        let mut sink = sink;
        let mut rbuf = vec![0u8; 256 * 1024];
        let mut got: u64 = 0;
        while got < bytes {
            let n = sink
                .read(&mut rbuf)
                .await
                .map_err(|e| format!("sink 读取失败: {e}"))?;
            if n == 0 {
                return Err(format!("sink 提前 EOF({got}/{bytes})"));
            }
            got += n as u64;
            while (per_mib.len() as u64) < got / MIB && (per_mib.len() as u64) < mib {
                per_mib.push(serde_json::json!({
                    "mi": per_mib.len() + 1,
                    "t_s": fin(t0.elapsed().as_secs_f64(), "t_s")?,
                    "fill": fill_bytes.load(Ordering::Relaxed),
                    "drain": got,
                    "eagain": 0, // stdout 不可得泵内部计数(见文件头声明)
                    "pending": 0,
                }));
            }
        }
        let elapsed_s = t0.elapsed().as_secs_f64();
        // sink 半关闭 → 对向(静默方向)泵 EOF → relay 双泵自然结束
        sink.shutdown().await.map_err(|e| e.to_string())?;
        let join_deadline = Duration::from_secs(60.max(mib / 4));
        tokio::time::timeout(join_deadline, relay)
            .await
            .map_err(|_| "relay 未在期限内收敛".to_string())?
            .map_err(|e| e.to_string())??;
        writer
            .await
            .map_err(|e| format!("writer join: {e}"))?
            .map_err(|e| format!("source 写入失败: {e}"))?;
        let (fb, fc) = (
            fill_bytes.load(Ordering::Relaxed),
            fill_calls.load(Ordering::Relaxed),
        );
        if fb != bytes || got != bytes {
            return Err(format!(
                "计量不一致: fill_bytes={fb} sink_bytes={got} 期望={bytes}"
            ));
        }
        let gbps = fin(bytes as f64 * 8.0 / elapsed_s / 1e9, "gbps")?;
        let (pct, user_s, sys_s) = match (cpu0, cpu_ticks()) {
            (Some((u0, s0)), Some((u1, s1))) => {
                let clk = clk_tck();
                let du = (u1 - u0) as f64 / clk;
                let ds = (s1 - s0) as f64 / clk;
                (
                    fin((du + ds) / elapsed_s * 100.0, "cpu_pct")?,
                    fin(du, "user_s")?,
                    fin(ds, "sys_s")?,
                )
            }
            _ => (0.0, 0.0, 0.0), // /proc 不可读时如实填 0(不冒充)
        };
        // 小包低负载阶段(独立 relay,不与大流量混跑)
        let (p50_us, p99_us) = small_packet_stage().await?;
        Ok(serde_json::json!({
            "schema": "fixed-ab-v1",
            "direction": direction,
            "mib": mib,
            "bytes": bytes,
            "elapsed_s": fin(elapsed_s, "elapsed_s")?,
            "gbps": gbps,
            "cpu": {
                "process_pct_single_core": pct,
                "user_s": user_s,
                "sys_s": sys_s,
            },
            "counters": {
                "fill_calls": fc,
                "fill_bytes": fb,
                "drain_calls": 0, // 单 DirHook 单点位不可观测(文件头声明),不冒充
                "drain_bytes": got,
                "eagain_fill_input_empty": 0,
                "eagain_drain_pipe_full": 0,
                "eagain_drain_write_full": 0,
                "pending_waits_read": 0,
                "pending_waits_write": 0,
                "coop_yields": 0,
            },
            "per_mib": per_mib,
            "small_packet": {
                "count": SMALL_COUNT,
                "size": SMALL_SIZE,
                "p50_us": p50_us,
                "p99_us": p99_us,
            },
        }))
    }
}
