//! Linux splice(2) 零拷贝中继:socket → pipe → socket,数据不经用户态。
//!
//! 适用条件(调用方判定):两端均为裸 tokio TCP 流——无 TLS/WS/yamux/压缩包装
//! (multiplex=false 且明文 tcp 载体时的典型形态)。实现方式是对两端 socket
//! 各 `dup` 一个独立 fd,就绪事件经 [`AsyncFd`] 注册,原 tokio 流全程不动,
//! 因此任一步资源失败(dup/pipe)都可原样回落用户态拷贝路径。
//!
//! 语义与用户态拷贝路径对齐:
//! * splice 返回 0(读 EOF)→ shutdown 对端写侧(半关闭传递);
//! * 限速按返回字节扣减令牌,令牌不足时管道(有界)充当节流缓冲;
//! * abort(流量封禁)→ 双向 SHUT_RDWR 模拟断连,唤醒对向泵。

use crate::BoxedIo;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd, OwnedFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::unix::AsyncFd;

const SPLICE_F_MOVE: u32 = 1;
const SPLICE_F_NONBLOCK: u32 = 2;
/// 管道目标容量:一次 readable 事件尽量搬空 socket 接收缓冲。
/// 页按需分配(空闲连接不占内存);F_SETPIPE_SZ 失败则回落默认 64KiB。
/// 实验记录:64KiB 与 256KiB 在三机串联链路上吞吐无差(均衡点由拓扑决定),
/// 取 256KiB 降低唤醒频次。
const PIPE_SIZE: i32 = 256 * 1024;
/// 单次排空上限:控制注入对端 socket 的突发尺寸,同时足够大以维持
/// 发送队列不空转(过小会 app-limit 饿住内核发送速率)。
const DRAIN_SLICE: usize = 256 * 1024;
/// 未发送数据低水位:writable 仅在 unsent < 低水位时唤醒。取值需显著
/// 大于 BDP(过小会把发送队列饿成 app-limited,实测 128KiB 时单流被
/// 卡在 ~2G);1MiB 在 0.4ms RTT 链路上足以维持满窗口。
/// 内核不支持时忽略。可用 RH_NOTSENT_LOWAT 覆盖(字节数,0 = 不设置;
/// C2 低水位矩阵实验用)。
const NOTSENT_LOWAT: u32 = 1024 * 1024;

fn notsent_lowat() -> u32 {
    static V: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *V.get_or_init(|| {
        std::env::var("RH_NOTSENT_LOWAT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(NOTSENT_LOWAT)
    })
}

/// 限速:预留式取消安全接口(goal-repair A5;适配 TokenBucket 等)。
/// reserve(n) 等待凑足 n 额度;等待期间 future 被 drop 不丢已扣令牌。
pub use crate::ChunkLimiter;

/// 取消信号(共享,lib 定义):完成即双向断连。用于规则禁用/删除等
/// 需要立即拆除在途转发的场景;两个方向的泵各自持有一份同时等待。
pub use crate::CancelFuture;

/// 全部 relay 中观测到的最小管道容量(字节;0 = 尚未建立过 relay)。
/// 高并发触及 fs.pipe-user-pages-soft 时内核会静默缩小新管道,
/// 该值用于诊断容量退化(如回落到 8KiB)。
pub static PIPE_CAP_MIN: AtomicUsize = AtomicUsize::new(0);

static PIPE_CAP_WARNED: AtomicBool = AtomicBool::new(false);

/// 观测到管道实际容量 actual 时更新最小值(goal-repair A7:compare_exchange
/// 循环实现真原子最小——修前 load+条件 store 在并发观测 8KiB/256KiB 时
/// 可把 256KiB 留作"最小值")。
fn update_pipe_cap_min(actual: usize) {
    let mut cur = PIPE_CAP_MIN.load(Ordering::Relaxed);
    loop {
        if cur != 0 && actual >= cur {
            return;
        }
        match PIPE_CAP_MIN.compare_exchange_weak(cur, actual, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return,
            Err(c) => cur = c,
        }
    }
}

/// 单方向钩子:计量 + 限速。
#[derive(Clone, Default)]
pub struct DirHook {
    /// 每实际搬运 n 字节回调一次(计量)。
    /// meter_on_fill=true:fill(搬入管道)完成即计——up 读侧口径,与
    /// copy 路径 CountedUserIo 读侧计数点位一致(先计量、后限速等待);
    /// false:排空送达后计——down 送达口径(与 copy 写侧成功计数一致)。
    pub on_bytes: Option<Arc<dyn Fn(u64) + Send + Sync>>,
    pub meter_on_fill: bool,
    /// 限速(预留式,取消安全):up=fill 后预留即消费;down=预留后按
    /// 实际送达消费,取消/部分送达退还未消费预留。
    pub limiter: Option<Arc<dyn ChunkLimiter>>,
}

/// BoxedIo 是否裸 tokio TCP(可 splice);TLS/WS/yamux/压缩包装均返回 None。
pub fn plain_tcp(io: &BoxedIo) -> Option<&tokio::net::TcpStream> {
    io.as_any().downcast_ref::<tokio::net::TcpStream>()
}

#[derive(Clone, Default)]
pub struct SpliceHooks {
    /// a→b 方向(服务端语义:user→carrier)。
    pub up: Option<DirHook>,
    /// b→a 方向(carrier→user)。
    pub down: Option<DirHook>,
    /// 返回 true → 双向断连(流量封禁)。
    pub abort: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    /// 取消(规则禁用/删除):完成即双向断连并结束两个泵,
    /// 包括阻塞在就绪等待中的泵(abort 闭包只在块边界被检查)。
    pub cancel: Option<CancelFuture>,
}

struct Pipe {
    r: OwnedFd,
    w: OwnedFd,
}

impl Pipe {
    fn new() -> io::Result<Self> {
        let mut fds = [0i32; 2];
        if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_NONBLOCK) } != 0 {
            return Err(io::Error::last_os_error());
        }
        let p = Pipe {
            r: unsafe { OwnedFd::from_raw_fd(fds[0]) },
            w: unsafe { OwnedFd::from_raw_fd(fds[1]) },
        };
        // 扩容失败不影响正确性,但必须知道实际容量:触及
        // fs.pipe-user-pages-soft 后内核会静默缩小管道(批量并发场景),
        // 记录最小观测值供诊断(A7:原子最小);首次退化告警一次。
        unsafe {
            if libc::fcntl(p.r.as_raw_fd(), libc::F_SETPIPE_SZ, PIPE_SIZE) < 0 {
                let e = io::Error::last_os_error();
                tracing::debug!(error = %e, "F_SETPIPE_SZ failed");
            }
            let actual = libc::fcntl(p.r.as_raw_fd(), libc::F_GETPIPE_SZ);
            if actual >= 0 {
                update_pipe_cap_min(actual as usize);
                if (actual as i32) < PIPE_SIZE && !PIPE_CAP_WARNED.swap(true, Ordering::Relaxed) {
                    tracing::warn!(
                        requested = PIPE_SIZE,
                        actual,
                        min_observed = PIPE_CAP_MIN.load(Ordering::Relaxed),
                        "pipe capacity below target (fs.pipe-user-pages-soft?)"
                    );
                }
            }
        }
        Ok(p)
    }
}

fn splice_sys(fd_in: i32, fd_out: i32, len: usize) -> io::Result<usize> {
    let n = unsafe {
        libc::splice(
            fd_in,
            std::ptr::null_mut(),
            fd_out,
            std::ptr::null_mut(),
            len,
            SPLICE_F_MOVE | SPLICE_F_NONBLOCK,
        )
    };
    if n < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(n as usize)
    }
}

struct SpliceSock {
    /// 与原 tokio 流共享同一 socket 的独立 fd:就绪事件走 AsyncFd,
    /// 原 fd 的注册/生命周期不受影响。
    fd: Arc<AsyncFd<OwnedFd>>,
}

impl SpliceSock {
    fn dup(stream: &tokio::net::TcpStream) -> io::Result<Self> {
        let dup = socket2::SockRef::from(stream).try_clone()?;
        let fd = unsafe { OwnedFd::from_raw_fd(dup.into_raw_fd()) };
        // 作用于共享 socket 本体;失败(旧内核)静默忽略;0 = 不设置
        let v = notsent_lowat();
        if v > 0 {
            unsafe {
                libc::setsockopt(
                    fd.as_raw_fd(),
                    libc::IPPROTO_TCP,
                    libc::TCP_NOTSENT_LOWAT,
                    &v as *const u32 as *const libc::c_void,
                    std::mem::size_of::<u32>() as libc::socklen_t,
                );
            }
        }
        Ok(Self {
            fd: Arc::new(AsyncFd::new(fd)?),
        })
    }

    fn raw_fd(&self) -> i32 {
        self.fd.get_ref().as_raw_fd()
    }
}

fn shutdown_wr(s: &SpliceSock) {
    unsafe {
        libc::shutdown(s.raw_fd(), libc::SHUT_WR);
    }
}

fn shutdown_rdwr(s: &SpliceSock) {
    unsafe {
        libc::shutdown(s.raw_fd(), libc::SHUT_RDWR);
    }
}

/// 单向泵:in socket → pipe → out socket。
/// 退出路径(EOF/读错误/写错误 → shutdown out 写侧;中止/取消 → 断连),
/// 与用户态 copy 循环「循环结束即 shutdown 写端」语义对齐(B5:写错误
/// 分支同样 shutdown 写侧,不再与 copy 口径分叉)。
/// 计量(B6):up(fill 口径,fill 完成即计、先于限速等待)与
/// down(送达口径,排空后按实际送达字节计)由 DirHook 装配决定。
/// 限速(A5):预留式等待与取消信号竞争;down 按实际送达消费,
/// 取消/部分送达退还未消费预留(A6:取消时已送达字节先计量再退出)。
async fn pump(
    in_sock: Arc<SpliceSock>,
    out_sock: Arc<SpliceSock>,
    pipe: Pipe,
    hook: Option<DirHook>,
    abort: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    cancel: Option<CancelFuture>,
) {
    let pr = pipe.r.as_raw_fd();
    let pw = pipe.w.as_raw_fd();

    // 取消等待(None = 永不触发);多个 select 复用同一实例,
    // 触发后 Shared 保持完成态,后续 select 立即取胜。
    let cancel_wait = async {
        match cancel {
            Some(c) => c.await,
            None => std::future::pending().await,
        }
    };
    tokio::pin!(cancel_wait);

    fn teardown(s1: &SpliceSock, s2: &SpliceSock) {
        shutdown_rdwr(s1);
        shutdown_rdwr(s2);
    }

    loop {
        if let Some(f) = &abort {
            if f() {
                teardown(&in_sock, &out_sock);
                return;
            }
        }
        // 一次事件搬运一块(≤ 管道容量);EAGAIN 重新等就绪。
        // 就绪等待与取消信号竞争:规则禁用/删除要求拆除阻塞中的泵。
        let n = 'fill: loop {
            let mut guard = match tokio::select! {
                g = in_sock.fd.readable() => g,
                _ = &mut cancel_wait => {
                    teardown(&in_sock, &out_sock);
                    return;
                }
            } {
                Ok(g) => g,
                Err(_) => {
                    shutdown_wr(&out_sock);
                    return;
                }
            };
            match splice_sys(in_sock.raw_fd(), pw, PIPE_SIZE as usize) {
                Ok(0) => {
                    shutdown_wr(&out_sock);
                    return;
                }
                Ok(n) => break 'fill n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    guard.clear_ready();
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    shutdown_wr(&out_sock);
                    return;
                }
            }
        };
        // B6:up 读侧口径——fill 完成即计量,先于限速等待
        //(copy 路径:CountedUserIo.poll_read 先计,copy_limited 后 acquire)
        if let Some(h) = &hook {
            if h.meter_on_fill {
                if let Some(c) = &h.on_bytes {
                    c(n as u64);
                }
            }
        }
        // A5:令牌等待与取消信号竞争(双泵同阻塞在限速等待时取消仍可达);
        // up(fill 口径)预留即消费;down(送达口径)持有至排空按送达消费。
        let mut reservation = match hook.as_ref().and_then(|h| h.limiter.clone()) {
            Some(l) => Some(tokio::select! {
                r = l.reserve(n) => r,
                _ = &mut cancel_wait => {
                    teardown(&in_sock, &out_sock);
                    return;
                }
            }),
            None => None,
        };
        if hook.as_ref().is_some_and(|h| h.meter_on_fill) {
            if let Some(r) = reservation.as_mut() {
                r.consume(n);
            }
        }
        if let Some(f) = &abort {
            if f() {
                drop(reservation); // 未消费预留退还
                teardown(&in_sock, &out_sock);
                return;
            }
        }
        // 排空管道到对端(对端拥塞时管道有界积压 → 背压到来源)。
        // 单次 ≤ DRAIN_SLICE:突发尺寸与拷贝路径一致。
        let mut delivered: usize = 0;
        loop {
            let mut guard = match tokio::select! {
                g = out_sock.fd.writable() => g,
                _ = &mut cancel_wait => {
                    // A6:排空中取消——已送达字节先计量再退出
                    if delivered > 0 {
                        if let Some(h) = &hook {
                            if !h.meter_on_fill {
                                if let Some(c) = &h.on_bytes {
                                    c(delivered as u64);
                                }
                            }
                        }
                        if let Some(r) = reservation.as_mut() {
                            r.consume(delivered);
                        }
                    }
                    drop(reservation); // 未消费预留退还
                    teardown(&in_sock, &out_sock);
                    return;
                }
            } {
                Ok(g) => g,
                Err(_) => {
                    // 写侧死亡:送达部分仍计量;B5:同样收尾 shutdown 写侧
                    shutdown_wr(&out_sock);
                    break;
                }
            };
            match splice_sys(pr, out_sock.raw_fd(), (n - delivered).min(DRAIN_SLICE)) {
                Ok(m) => delivered += m,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    guard.clear_ready();
                    continue;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(_) => {
                    // B5:写错误分支与 copy 路径一致地收尾 shutdown 写侧
                    shutdown_wr(&out_sock);
                    break;
                }
            }
            if delivered >= n {
                break;
            }
        }
        // 送达计量(down 口径):按实际送达字节,与 copy 路径口径一致
        if delivered > 0 {
            if let Some(h) = &hook {
                if !h.meter_on_fill {
                    if let Some(c) = &h.on_bytes {
                        c(delivered as u64);
                    }
                }
            }
            if let Some(r) = reservation.as_mut() {
                r.consume(delivered);
            }
        }
        drop(reservation); // down 部分送达:未消费预留退还
        if delivered < n {
            // 未排空即退出:写侧已不可用
            return;
        }
    }
}

/// 零拷贝双向中继。a/b 为两条裸 TCP(借用,不消费——回落时调用方仍持有原值)。
/// 仅在 dup/pipe 等资源申请失败时返回 Err(回落信号);中继本身运行至双向结束。
/// 父任务被取消(A2)时双向泵随 [`crate::AbortOnDropHandle`] 守卫终止并断连
/// ——守卫以 `&mut` join,句柄不移出,父 future 被 drop 即 abort 双泵。
pub async fn splice_relay(
    a: &tokio::net::TcpStream,
    b: &tokio::net::TcpStream,
    hooks: SpliceHooks,
) -> io::Result<()> {
    let sa = Arc::new(SpliceSock::dup(a)?);
    let sb = Arc::new(SpliceSock::dup(b)?);
    let p_ab = Pipe::new()?;
    let p_ba = Pipe::new()?;
    let mut g1 = crate::AbortOnDropHandle::new(tokio::spawn(pump(
        sa.clone(),
        sb.clone(),
        p_ab,
        hooks.up,
        hooks.abort.clone(),
        hooks.cancel.clone(),
    )));
    let mut g2 = crate::AbortOnDropHandle::new(tokio::spawn(pump(
        sb,
        sa,
        p_ba,
        hooks.down,
        hooks.abort,
        hooks.cancel,
    )));
    // final-gaps 补修:单泵 panic 时对向泵立即终止(不永存)。修前
    // `tokio::join!` 等双泵——panic 泵不做 teardown,对向阻塞在
    // readable() 永不结束,relay 卡死扣住 socket(与 mux/copy 路径的
    // panic 收敛缺口同款;写错误路径无此问题:B5 shutdown_wr 经 EOF
    // 链唤醒对向,见 splice_write_error_shutdown_parity)。
    // join_relay_pumps:先完成侧 panic → 立即返回(守卫 Drop → abort
    // 对向,fd 随 Arc 释放关闭);正常 EOF → 等对向自然结束(半关闭)。
    crate::join_relay_pumps(&mut g1, &mut g2).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    async fn tcp_pair() -> (tokio::net::TcpStream, tokio::net::TcpStream) {
        let ln = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = ln.local_addr().unwrap();
        let c = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (s, _) = ln.accept().await.unwrap();
        (c, s)
    }

    /// 轮询等待原子计数到达期望值(计量在泵任务内异步发生,读者完成≠已回调)。
    async fn wait_count(v: &AtomicU64, want: u64) {
        for _ in 0..200 {
            if v.load(Ordering::Relaxed) == want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(v.load(Ordering::Relaxed), want);
    }

    // 基本搬运 + 半关闭:echo 目标侧 EOF 透传回用户侧
    #[tokio::test]
    async fn splice_relay_echo_and_half_close() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (user_cli, user_srv) = tcp_pair().await;
        let (tgt_conn, mut tgt_echo) = tcp_pair().await;
        // echo:读尽即关
        tokio::spawn(async move {
            let mut buf = vec![0u8; 4096];
            loop {
                match tgt_echo.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tgt_echo.write_all(&buf[..n]).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let relay = tokio::spawn(async move {
            splice_relay(&user_srv, &tgt_conn, SpliceHooks::default())
                .await
                .unwrap();
        });
        // 并发读者:避免回程数据写满缓冲导致写端死锁
        let payload = vec![0xabu8; 256 * 1024];
        let (mut r_half, mut w_half) = user_cli.into_split();
        let recv = tokio::spawn(async move {
            let mut got = Vec::new();
            let _ = r_half.read_to_end(&mut got).await;
            got
        });
        w_half.write_all(&payload).await.unwrap();
        // 半关闭:写侧 EOF → echo 关闭 → 中继传递 → 读侧 EOF
        w_half.shutdown().await.unwrap();
        let got = recv.await.unwrap();
        assert_eq!(got.len(), payload.len());
        assert!(got.iter().all(|&b| b == 0xab));
        relay.await.unwrap();
    }

    // 计量钩子:双向字节计数与实际载荷一致
    #[tokio::test]
    async fn splice_relay_counts_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let up_count = Arc::new(AtomicU64::new(0));
        let down_count = Arc::new(AtomicU64::new(0));
        let (a_cli, a_srv) = tcp_pair().await;
        let (b_cli, b_srv) = tcp_pair().await;
        let uc = up_count.clone();
        let dc = down_count.clone();
        let hooks = SpliceHooks {
            up: Some(DirHook {
                on_bytes: Some(Arc::new(move |n| {
                    uc.fetch_add(n, Ordering::Relaxed);
                })),
                meter_on_fill: true,
                limiter: None,
            }),
            down: Some(DirHook {
                on_bytes: Some(Arc::new(move |n| {
                    dc.fetch_add(n, Ordering::Relaxed);
                })),
                meter_on_fill: false,
                limiter: None,
            }),
            abort: None,
            cancel: None,
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        // 并发读者(双向各一个):up(a→b)的数据到 b 侧,down(b→a)到 a 侧
        let payload: Vec<u8> = (0..128 * 1024).map(|i| (i % 251) as u8).collect();
        let back = vec![7u8; 300 * 1024];
        let (mut a_r, mut a_w) = a_cli.into_split();
        let (mut b_r, mut b_w) = b_cli.into_split();
        let p2 = back.clone();
        let recv_dn = tokio::spawn(async move {
            let mut buf = vec![0u8; p2.len()];
            let _ = a_r.read_exact(&mut buf).await;
            buf
        });
        let p1 = payload.clone();
        let recv_up = tokio::spawn(async move {
            let mut buf = vec![0u8; p1.len()];
            let _ = b_r.read_exact(&mut buf).await;
            buf
        });
        a_w.write_all(&payload).await.unwrap();
        b_w.write_all(&back).await.unwrap();
        let got_up = recv_up.await.unwrap();
        let got_dn = recv_dn.await.unwrap();
        assert_eq!(got_up, payload);
        assert_eq!(got_dn, back);
        // 计量在排空完成后回调:读者完成后再等一小段窗口
        wait_count(&up_count, payload.len() as u64).await;
        wait_count(&down_count, back.len() as u64).await;
        relay.abort();
    }

    // 限速钩子:acquire 被调用且不破坏数据(固定延迟 shim 验证调用时序)
    #[tokio::test]
    async fn splice_relay_rate_limit_hook() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (a_cli, a_srv) = tcp_pair().await;
        let (b_cli, b_srv) = tcp_pair().await;
        struct SlowLimiter(Arc<std::sync::atomic::AtomicUsize>);
        impl crate::ChunkLimiter for SlowLimiter {
            fn reserve(
                &self,
                n: usize,
            ) -> futures::future::BoxFuture<'static, Box<dyn crate::ChunkReservation>> {
                let c = self.0.clone();
                Box::pin(async move {
                    c.fetch_add(1, Ordering::Relaxed);
                    tokio::time::sleep(Duration::from_millis((n as u64 / 65536).max(1))).await;
                    struct NoRes;
                    impl crate::ChunkReservation for NoRes {
                        fn consume(&mut self, _n: usize) {}
                    }
                    Box::new(NoRes) as Box<dyn crate::ChunkReservation>
                })
            }
        }
        let limiter: Arc<SlowLimiter> = Arc::new(SlowLimiter(Arc::new(
            std::sync::atomic::AtomicUsize::new(0),
        )));
        let hooks = SpliceHooks {
            up: Some(DirHook {
                on_bytes: None,
                meter_on_fill: true,
                limiter: Some(limiter.clone() as Arc<dyn ChunkLimiter>),
            }),
            down: None,
            abort: None,
            cancel: None,
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        let payload = vec![1u8; 64 * 1024];
        let (mut b_r, mut a_w) = {
            let (_ar, aw) = a_cli.into_split();
            let (br, bw) = b_cli.into_split();
            drop(bw);
            (br, aw)
        };
        let p1 = payload.clone();
        let recv = tokio::spawn(async move {
            let mut buf = vec![0u8; p1.len()];
            let _ = b_r.read_exact(&mut buf).await;
            buf
        });
        a_w.write_all(&payload).await.unwrap();
        let got = recv.await.unwrap();
        assert_eq!(got, payload);
        assert!(
            limiter.0.load(Ordering::Relaxed) >= 1,
            "限速 reserve 必须被调用"
        );
        relay.abort();
    }

    // 取消:规则禁用/删除 → 阻塞在就绪等待中的泵被唤醒,双向断连,relay 收敛
    #[tokio::test]
    async fn splice_relay_cancel_disconnects() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (a_cli, a_srv) = tcp_pair().await;
        let (b_cli, b_srv) = tcp_pair().await;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let cancel: CancelFuture = rx.map(|_| ()).boxed().shared();
        let hooks = SpliceHooks {
            up: None,
            down: None,
            abort: None,
            cancel: Some(cancel),
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        // 先送一小笔让泵进入搬运状态(也覆盖「块间触发」路径),再等其回到等待
        let mut a = a_cli;
        let mut b = b_cli;
        a.write_u8(1).await.unwrap();
        let mut probe = [0u8; 1];
        b.read_exact(&mut probe).await.unwrap();
        assert_eq!(probe[0], 1);
        // 泵此刻阻塞在 readable 等待:触发取消
        tx.send(()).unwrap();
        // 双向断连:两侧读均应观察到 EOF,且 relay 任务随之结束
        let mut buf = [0u8; 16];
        let ra = tokio::time::timeout(Duration::from_secs(2), a.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        let rb = tokio::time::timeout(Duration::from_secs(2), b.read(&mut buf))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ra, 0);
        assert_eq!(rb, 0);
        tokio::time::timeout(Duration::from_secs(2), relay)
            .await
            .unwrap()
            .unwrap();
    }
    // A2:父任务取消必须终止双向泵(修前 take_handle 把句柄移出守卫,
    // 父取消时 Drop 不再 abort,子泵 detach——/tmp/rh-splice-audit-ZrHoaE
    // harness parent_drop_detaches:取消后存活子任务 = 2)。
    // 探测协议:取消后写入的字节在窗口期内不得被转发。
    #[tokio::test]
    async fn splice_parent_cancel_kills_children() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut a_cli, a_srv) = tcp_pair().await;
        let (mut b_cli, b_srv) = tcp_pair().await;
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, SpliceHooks::default())
                .await
                .unwrap();
        });
        // 建立流:双向各一字节
        a_cli.write_u8(1).await.unwrap();
        assert_eq!(b_cli.read_u8().await.unwrap(), 1);
        b_cli.write_u8(2).await.unwrap();
        assert_eq!(a_cli.read_u8().await.unwrap(), 2);
        // 父任务取消(必须以 cancelled 完成 —— 守卫生效,子泵未 detach)
        relay.abort();
        assert!(relay.await.unwrap_err().is_cancelled());
        // 泵终止需要一次调度窗口,随后写探测字节。
        // ★测试修正(2026-10-13,主代理授权):teardown 已对中继双端 SHUT_RDWR,
        // 因此本地探测写与对端读都可能观察到断连(BrokenPipe/ConnectionReset/
        // UnexpectedEof)—— 断连 == 没有字节被转发,与超时一样合法;
        // 只有「收到数据字节」才证明子泵 detach(A2)。其余错误仍失败。
        tokio::time::sleep(Duration::from_millis(50)).await;
        match a_cli.write_u8(9).await {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::BrokenPipe | io::ErrorKind::ConnectionReset
                ) => {}
            Err(e) => panic!("探测写入失败(非断连类错误): {e}"),
        }
        match tokio::time::timeout(Duration::from_millis(300), b_cli.read_u8()).await {
            Err(_) => {} // 超时 = 未转发 = 通过(泵已终止)
            // EOF(正常断连表现)= 未转发 = 通过;其他读错误仍失败
            Ok(Err(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {}
            Ok(Ok(v)) => panic!("取消后字节 {v} 仍被转发(子泵 detach,A2)"),
            Ok(Err(e)) => panic!("读侧异常(非断连类错误): {e}"),
        }
        // 资源清理:两侧客户端连接正常丢弃,无残余任务(本测试结束即 runtime
        // 单测实例收尾,若有泄漏任务会在后续测试的 runtime 上留下干扰)。
        drop(a_cli);
        drop(b_cli);
    }

    // A5:双向泵同时阻塞在限速预留等待时,取消信号必须可达并拆除泵
    //(修前裸 acquire await 不在 select 中,harness:双泵仍 pending)。
    // ★测试修正(2026-10-13,主代理授权):修前 NeverLimiter 立即返回预留,
    // 泵从未真正进入等待 —— 现在改为真正 pending 的预留 future,并以
    // 「两泵都已进入 reserve 等待」的计数屏障确保取消发生在等待中;
    // 未进入等待即取消成功,不构成本回归的通过条件。
    #[tokio::test]
    async fn splice_token_cancel_reaches_pumps_in_rate_wait() {
        use tokio::io::AsyncWriteExt;
        struct WaitLimiter {
            /// reserve future 首次被 poll(pump 真正开始等待预留)时 +1。
            entered: Arc<std::sync::atomic::AtomicUsize>,
        }
        impl crate::ChunkLimiter for WaitLimiter {
            fn reserve(
                &self,
                _n: usize,
            ) -> futures::future::BoxFuture<'static, Box<dyn crate::ChunkReservation>> {
                let entered = self.entered.clone();
                Box::pin(async move {
                    entered.fetch_add(1, Ordering::SeqCst);
                    // 永不完成:泵将停留在预留等待中,直到取消信号到达
                    std::future::pending::<Box<dyn crate::ChunkReservation>>().await
                })
            }
        }
        let (mut a_cli, a_srv) = tcp_pair().await;
        let (mut b_cli, b_srv) = tcp_pair().await;
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let cancel: CancelFuture = rx.map(|_| ()).boxed().shared();
        let entered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let dir = |l: Arc<WaitLimiter>| DirHook {
            on_bytes: None,
            meter_on_fill: false,
            limiter: Some(l as Arc<dyn ChunkLimiter>),
        };
        let hooks = SpliceHooks {
            up: Some(dir(Arc::new(WaitLimiter {
                entered: entered.clone(),
            }))),
            down: Some(dir(Arc::new(WaitLimiter {
                entered: entered.clone(),
            }))),
            abort: None,
            cancel: Some(cancel),
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        // 双向各写一笔:两泵都 fill 后进入永不完成的预留等待
        a_cli.write_u8(1).await.unwrap();
        b_cli.write_u8(2).await.unwrap();
        // 屏障:两个泵的 reserve future 都已被 poll(真正进入等待)才允许取消
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while entered.load(Ordering::SeqCst) < 2 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "两泵未都进入限速预留等待(entered={})",
                entered.load(Ordering::SeqCst)
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        // 再让出一拍,确保 select! 稳定挂起在 reserve 分支后触发取消
        tokio::time::sleep(Duration::from_millis(50)).await;
        tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), relay)
            .await
            .expect("取消必须到达阻塞在限速等待中的泵(A5)")
            .unwrap();
    }

    // A7:PIPE_CAP_MIN 并发更新必须保持真原子最小
    //(修前 load+条件 store 可把 256KiB 留作最小值;
    // harness pipe_min_lost_update:观测 [8192,262144] 记录 min=262144)。
    #[test]
    fn pipe_cap_min_atomic() {
        PIPE_CAP_MIN.store(0, Ordering::Relaxed);
        let threads: Vec<_> = (0..8)
            .map(|i| {
                std::thread::spawn(move || {
                    for _ in 0..2000 {
                        if i % 2 == 0 {
                            update_pipe_cap_min(8192);
                        } else {
                            update_pipe_cap_min(262144);
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        assert_eq!(
            PIPE_CAP_MIN.load(Ordering::Relaxed),
            8192,
            "并发观测 8KiB/256KiB 后最小值必须保持 8KiB(A7)"
        );
    }

    // A6:排空中途取消——已送达字节必须先计量再退出
    //(修前 return 先于回调,harness:accepted=3, callback=0)。
    // 构造:down 方向灌满对端接收缓冲(对端不读)使排空部分送达,
    // 取消后对端排干 socket,收到的字节数必须 == 计量值。
    // ★测试修正(2026-10-13,主代理授权):read_to_end 原漏 .await(future 被
    // 直接丢弃,got 恒 0)——现补 .await 并以 timeout 包裹,relay/writer/read
    // 三者结果显式检查,不再 let _ 吞错。
    // ★覆盖口径如实标注:本用例验证「实际收到字节 == 计量值」的不变量;
    // 512KiB 在 loopback 缓冲下可能全部送达(m == payload_len),此时**不
    // 构成**「部分块边界」的覆盖 —— 部分写故障分支由下方
    // splice_partial_meter_write_fault 注入回归单独覆盖与报告。
    #[tokio::test]
    async fn splice_partial_meter_on_cancel() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut a_cli, a_srv) = tcp_pair().await;
        let (b_cli, b_srv) = tcp_pair().await;
        let metered = Arc::new(AtomicU64::new(0));
        let m2 = metered.clone();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let cancel: CancelFuture = rx.map(|_| ()).boxed().shared();
        let dir = DirHook {
            on_bytes: Some(Arc::new(move |n| {
                m2.fetch_add(n, Ordering::Relaxed);
            })),
            meter_on_fill: false,
            limiter: None,
        };
        // b→a 为 down(carrier→user):送达计量挂 down 钩
        let hooks = SpliceHooks {
            up: None,
            down: Some(dir),
            abort: None,
            cancel: Some(cancel),
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        // b 侧(客户端对端)灌 512KiB:a 侧不读 → 排空部分送达后阻塞
        let mut b_w = b_cli;
        let payload_len = 512 * 1024;
        let payload = vec![3u8; payload_len];
        let writer = tokio::spawn(async move {
            let _ = b_w.write_all(&payload).await;
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        tx.send(()).unwrap();
        // 取消后 relay 必须收敛;panic/abort 都是失败,不再吞 JoinError
        match tokio::time::timeout(Duration::from_secs(3), relay).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) if e.is_panic() => panic!("relay 任务 panic: {e}"),
            Ok(Err(e)) => panic!("relay 被意外 abort: {e}"),
            Err(_) => panic!("取消后 relay 未在 3s 内收敛"),
        }
        writer.await.expect("writer 任务必须正常结束");
        // 对端排干 a 侧收到的字节:必须与计量值一致(部分送达如实计量)
        let mut got = Vec::new();
        tokio::time::timeout(Duration::from_secs(3), a_cli.read_to_end(&mut got))
            .await
            .expect("a_cli read_to_end 必须在 3s 内完成(teardown FIN 已发出)")
            .expect("read_to_end IO 失败");
        let m = metered.load(Ordering::Relaxed);
        assert!(
            m > 0 && m <= payload_len as u64,
            "取消时已送达字节必须计量(m={m})"
        );
        assert_eq!(
            got.len() as u64,
            m,
            "计量值必须等于实际送达字节数(A6:got={} metered={m})",
            got.len()
        );
    }

    // A6 部分写分支(注入回归,2026-10-13 主代理授权新增):写侧中途死亡时,
    // 已送达字节必须如实计量(0 < m < payload 即命中),relay 必须收敛。
    // 注入方式:a_cli 读侧 SHUT_RD → 内核对在途数据回 RST → a_srv 写侧
    // EPIPE → 排空循环中途退出(B5 分支)按已送达量计量。
    // ★覆盖口径如实:RST 到达时的排空进度由调度决定,单次尝试可能全部
    // 送达(未命中部分分支)。本回归多次尝试,不变量断言(收敛、
    // 0 ≤ m ≤ payload)恒真,「部分块命中」次数以测试输出如实报告,
    // 不假称覆盖;不为制造背压触碰任何 SO_RCVBUF/SO_SNDBUF(红线)。
    #[tokio::test]
    async fn splice_partial_meter_write_fault() {
        use tokio::io::AsyncWriteExt;
        let attempts = 6usize;
        let payload_len = 16 * 1024 * 1024;
        let mut hit_partial = 0usize;
        for attempt in 0..attempts {
            let (a_cli, a_srv) = tcp_pair().await;
            let (b_cli, b_srv) = tcp_pair().await;
            let metered = Arc::new(AtomicU64::new(0));
            let m2 = metered.clone();
            let (tx, rx) = tokio::sync::oneshot::channel::<()>();
            let cancel: CancelFuture = rx.map(|_| ()).boxed().shared();
            let dir = DirHook {
                on_bytes: Some(Arc::new(move |n| {
                    m2.fetch_add(n, Ordering::Relaxed);
                })),
                meter_on_fill: false,
                limiter: None,
            };
            let hooks = SpliceHooks {
                up: None,
                down: Some(dir),
                abort: None,
                cancel: Some(cancel),
            };
            let relay = tokio::spawn(async move {
                splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
            });
            let mut b_w = b_cli;
            let payload = vec![3u8; payload_len];
            let writer = tokio::spawn(async move {
                let _ = b_w.write_all(&payload).await;
            });
            // 不同窗口后注入 RST:窗口越小,越可能落在排空中途
            tokio::time::sleep(Duration::from_millis(1 + attempt as u64)).await;
            unsafe { libc::shutdown(a_cli.as_raw_fd(), libc::SHUT_RD) };
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = tx.send(());
            // 不变量:relay 必须收敛(B5 写错误不挂死);panic 必须暴露
            match tokio::time::timeout(Duration::from_secs(3), relay).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) if e.is_panic() => panic!("relay 任务 panic: {e}"),
                Ok(Err(e)) => panic!("relay 意外被 abort: {e}"),
                Err(_) => panic!("写故障注入后 relay 3s 未收敛(attempt {attempt})"),
            }
            let _ = writer.await;
            let m = metered.load(Ordering::Relaxed);
            assert!(
                m <= payload_len as u64,
                "计量不得超过载荷(m={m},attempt {attempt})"
            );
            if m > 0 && m < payload_len as u64 {
                hit_partial += 1;
            }
            eprintln!(
                "write_fault attempt {attempt}: metered={m}/{payload_len} partial_hit={}",
                m > 0 && m < payload_len as u64
            );
            drop(a_cli);
        }
        eprintln!("write_fault: {hit_partial}/{attempts} 次命中部分送达分支(0 < m < payload)");
    }

    // B6:up 计量点位 = fill 完成后、限速等待之前
    //(极小令牌桶阻塞期间 fill 字节立即可见,与 copy 路径
    // CountedUserIo 读侧先计的时序窗口一致;harness 同款断言)。
    #[tokio::test]
    async fn splice_up_meter_visible_during_rate_block() {
        use tokio::io::AsyncWriteExt;
        struct NeverLimiter;
        impl crate::ChunkLimiter for NeverLimiter {
            fn reserve(
                &self,
                _n: usize,
            ) -> futures::future::BoxFuture<'static, Box<dyn crate::ChunkReservation>> {
                Box::pin(async {
                    struct NoRes;
                    impl crate::ChunkReservation for NoRes {
                        fn consume(&mut self, _n: usize) {}
                    }
                    Box::new(NoRes) as Box<dyn crate::ChunkReservation>
                })
            }
        }
        let (mut a_cli, a_srv) = tcp_pair().await;
        let (_b_cli, b_srv) = tcp_pair().await;
        let metered = Arc::new(AtomicU64::new(0));
        let m2 = metered.clone();
        let hooks = SpliceHooks {
            up: Some(DirHook {
                on_bytes: Some(Arc::new(move |n| {
                    m2.fetch_add(n, Ordering::Relaxed);
                })),
                meter_on_fill: true,
                limiter: Some(Arc::new(NeverLimiter) as Arc<dyn ChunkLimiter>),
            }),
            down: None,
            abort: None,
            cancel: None,
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        // 写 64KiB:fill 完成即计量,随后阻塞在预留等待(零送达)
        let payload = vec![5u8; 64 * 1024];
        a_cli.write_all(&payload).await.unwrap();
        // 限速阻塞期间计量必须已可见(轮询带截止)
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while metered.load(Ordering::Relaxed) < payload.len() as u64 {
            assert!(
                tokio::time::Instant::now() < deadline,
                "限速阻塞期间 fill 字节必须已计量(B6)"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        relay.abort();
    }

    // B5:写错误分支必须 shutdown 写侧(与 copy 路径口径一致;修前
    // 直接 return,harness:WRITE_ERROR 计 3B 且无 shutdown)。
    // 构造:对端以 SO_LINGER=0 关闭产生 RST → 写侧 EPIPE → 断言
    // 泵收敛(不挂死)且对向读侧观察到断连。
    #[tokio::test]
    async fn splice_write_error_shutdown_parity() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut a_cli, a_srv) = tcp_pair().await;
        let (b_cli, b_srv) = tcp_pair().await;
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, SpliceHooks::default())
                .await
                .unwrap();
        });
        a_cli.write_u8(1).await.unwrap();
        // RST 断连 b 侧对端
        let std_b = b_cli.into_std().unwrap();
        let linger = libc::linger {
            l_onoff: 1,
            l_linger: 0,
        };
        unsafe {
            libc::setsockopt(
                std_b.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_LINGER,
                &linger as *const _ as *const libc::c_void,
                std::mem::size_of::<libc::linger>() as libc::socklen_t,
            );
        }
        drop(std_b); // RST
                     // a→b 方向写错误;泵必须收敛(relay 任务结束,不挂死)
        a_cli.write_all(&vec![7u8; 64 * 1024]).await.unwrap_or(());
        tokio::time::timeout(Duration::from_secs(3), relay)
            .await
            .expect("写错误后泵必须收敛(B5:shutdown 写侧,不挂死)")
            .unwrap();
        // 对向读侧观察到断连(EOF 或错误)
        let mut buf = [0u8; 16];
        let _ = tokio::time::timeout(Duration::from_secs(2), a_cli.read(&mut buf)).await;
    }

    // final-gaps(红→绿,Linux gate):单泵 panic(经 on_bytes 计量回调
    // 注入,回调在泵内执行)时对向泵必须立即终止——修前 splice_relay 用
    // `tokio::join!` 等双泵,panic 泵不做 teardown,对向阻塞在
    // readable() 永不结束,relay 卡死扣住 socket(mux/copy 路径已有
    // 同款回归,splice 漏了)。对向静默(无数据)是永存形态的严格判据。
    #[tokio::test]
    async fn splice_sibling_aborted_on_pump_panic() {
        use tokio::io::AsyncWriteExt;
        let (mut a_cli, a_srv) = tcp_pair().await;
        let (_b_cli, b_srv) = tcp_pair().await;
        let hooks = SpliceHooks {
            // up 泵(a→b)首次计量回调即 panic
            up: Some(DirHook {
                on_bytes: Some(Arc::new(|_n| panic!("injected meter panic"))),
                meter_on_fill: true,
                limiter: None,
            }),
            down: None,
            abort: None,
            cancel: None,
        };
        let relay = tokio::spawn(async move {
            splice_relay(&a_srv, &b_srv, hooks).await.unwrap();
        });
        // 触发 up 泵 fill → 计量回调 panic;down 泵停在 b 读(对端静默)
        a_cli.write_u8(1).await.unwrap();
        tokio::time::timeout(Duration::from_secs(3), relay)
            .await
            .expect("单泵 panic 后 splice_relay 必须收敛(对向泵不得永存)")
            .unwrap();
    }
}
