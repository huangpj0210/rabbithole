// refrelay2 —— targeted-before-round-3 公平性修正版诊断探针(非生产代码)。
// 相对 refrelay.rs 的修正(响应主代理裁定):
//   * 修正 1:可选 TCP_NODELAY(旧版未设置,如实先打印两端默认值);
//   * 修正 2:可选 TCP_NOTSENT_LOWAT 有限档(仅作用于本探针的 socket,
//     不触碰生产 carrier;红线仍禁 SO_RCVBUF/SO_SNDBUF——本探针同样不设);
//   * 修正 3:pipe 实际容量以 F_GETPIPE_SZ 实测报告(旧报告把"调用请求
//     256KiB"错称为实际容量);
//   * copy 读块 64KiB 上限(与红线一致)。
// 用法: refrelay2 <listen_port> <target_host:port> <copy|splice> [nodelay01] [lowat_bytes]
// 输出:启动行打印两 socket 的 TCP_NODELAY/TCP_NOTSENT_LOWAT/SO_SNDBUF/SO_RCVBUF
// 实际值(splice 模式另报 F_GETPIPE_SZ)。
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;

extern "C" {
    fn pipe(fds: *mut i32) -> i32;
    fn close(fd: i32) -> i32;
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn setsockopt(fd: i32, lvl: i32, opt: i32, val: *const i32, len: u32) -> i32;
    fn getsockopt(fd: i32, lvl: i32, opt: i32, val: *mut i32, len: *mut u32) -> i32;
    fn splice(fd_in: i32, off_in: *mut i64, fd_out: i32, off_out: *mut i64,
              len: usize, flags: u32) -> i64;
}

const SPLICE_F_MOVE: u32 = 1;
const IPPROTO_TCP: i32 = 6;
const TCP_NODELAY: i32 = 1;
const TCP_NOTSENT_LOWAT: i32 = 37; // linux/net/tcp.h
const SOL_SOCKET: i32 = 1;
const SO_SNDBUF: i32 = 7;
const SO_RCVBUF: i32 = 8;
const F_GETPIPE_SZ: i32 = 1032;

fn gso(fd: i32, lvl: i32, opt: i32) -> i32 {
    let mut v: i32 = 0;
    let mut l: u32 = 4;
    unsafe { getsockopt(fd, lvl, opt, &mut v, &mut l) };
    v
}

fn sso(fd: i32, lvl: i32, opt: i32, v: i32) -> bool {
    unsafe { setsockopt(fd, lvl, opt, &v, 4) == 0 }
}

#[derive(Clone)]
struct Cfg {
    nodelay: bool,
    lowat: u32,
}

fn tune(fd: i32, cfg: &Cfg) {
    if cfg.nodelay && !sso(fd, IPPROTO_TCP, TCP_NODELAY, 1) {
        eprintln!("TCP_NODELAY set failed");
    }
    if cfg.lowat > 0 && !sso(fd, IPPROTO_TCP, TCP_NOTSENT_LOWAT, cfg.lowat as i32) {
        eprintln!("TCP_NOTSENT_LOWAT={} set failed", cfg.lowat);
    }
}

fn report(tag: &str, fd: i32) {
    unsafe {
        println!("{tag}: TCP_NODELAY={} TCP_NOTSENT_LOWAT={} SO_SNDBUF={} SO_RCVBUF={}",
            gso(fd, IPPROTO_TCP, TCP_NODELAY),
            gso(fd, IPPROTO_TCP, TCP_NOTSENT_LOWAT),
            gso(fd, SOL_SOCKET, SO_SNDBUF),
            gso(fd, SOL_SOCKET, SO_RCVBUF));
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: {} <listen_port> <target> <copy|splice> [nodelay01] [lowat_bytes]", a[0]);
        std::process::exit(2);
    }
    let port: u16 = a[1].parse().expect("port");
    let target = a[2].clone();
    let mode = a[3].clone();
    let cfg = Cfg {
        nodelay: a.get(4).map(|s| s == "1").unwrap_or(false),
        lowat: a.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
    };
    assert!(mode == "copy" || mode == "splice");
    let ln = TcpListener::bind(("0.0.0.0", port)).expect("bind");
    // 探针 listener socket 自身上报(实际连接的选项在 accept 后 tune+report)
    println!("refrelay2 :{port} -> {target} mode={mode} nodelay={} lowat={} pid={}",
             cfg.nodelay as i32, cfg.lowat, std::process::id());
    report("listen-fd-default", ln.as_raw_fd());
    for stream in ln.incoming() {
        let s = match stream { Ok(s) => s, Err(_) => continue };
        let t = target.clone();
        let m = mode.clone();
        // 与旧 refrelay 相同结构:每连接独立线程跑 handle(主循环继续 accept;
        // 修正版曾把 handle 内联进主循环并 join,阻塞 accept 被实证 hang)
        let c2 = cfg.clone();
        std::thread::spawn(move || handle(s, t, m, &c2));
    }
}

fn handle(s: TcpStream, target: String, mode: String, cfg: &Cfg) {
    let upstream = match TcpStream::connect(&target) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("connect {target}: {e}");
            return;
        }
    };
    tune(s.as_raw_fd(), cfg);
    tune(upstream.as_raw_fd(), cfg);
    report("user-fd", s.as_raw_fd());
        report("upstream-fd", upstream.as_raw_fd());
        let u2 = s.try_clone().unwrap();
        let up2 = upstream.try_clone().unwrap();
        let m1 = mode.clone(); let m2 = mode.clone();
        // h1: user→upstream;h2: upstream→user(方向勿反,反了则双向缺一回路易 hang)
        let h1 = std::thread::spawn(move || pump(&u2, &upstream, &m1));
        let h2 = std::thread::spawn(move || pump(&up2, &s, &m2));
        let _ = h1.join(); let _ = h2.join();
}

fn pump(a: &TcpStream, b: &TcpStream, mode: &str) {
    match mode {
        "splice" => pump_splice(a, b),
        _ => pump_copy(a, b),
    }
}

fn pump_copy(a: &TcpStream, b: &TcpStream) {
    let mut a = a; let mut b = b;
    let mut buf = vec![0u8; 64 * 1024]; // 红线:读块 ≤64KiB
    loop {
        match a.read(&mut buf) {
            Ok(0) | Err(_) => { let _ = b.shutdown(Shutdown::Write); return; }
            Ok(n) => { if b.write_all(&buf[..n]).is_err() { return; } }
        }
    }
}

fn pump_splice(a: &TcpStream, b: &TcpStream) {
    unsafe {
        let mut fds = [0i32; 2];
        if pipe(fds.as_mut_ptr()) != 0 {
            eprintln!("pipe failed, fallback copy");
            return pump_copy(a, b);
        }
        // 只读实测管道实际容量(不设置容量,保持系统默认;F_SETPIPE_SZ 不用)
        let cap = fcntl(fds[0], F_GETPIPE_SZ);
        println!("pipe actual capacity (F_GETPIPE_SZ): {cap} bytes");
        loop {
            let n = splice(a.as_raw_fd(), std::ptr::null_mut(), fds[1], std::ptr::null_mut(),
                           256 * 1024, SPLICE_F_MOVE);
            if n <= 0 {
                let _ = shutdown_wr(b.as_raw_fd());
                close(fds[0]); close(fds[1]);
                return;
            }
            let mut done: i64 = 0;
            while done < n {
                let m = splice(fds[0], std::ptr::null_mut(), b.as_raw_fd(), std::ptr::null_mut(),
                               (n - done) as usize, SPLICE_F_MOVE);
                if m <= 0 { close(fds[0]); close(fds[1]); return; }
                done += m;
            }
        }
    }
}

extern "C" { fn shutdown(fd: i32, how: i32) -> i32; }
fn shutdown_wr(fd: i32) -> i32 { unsafe { shutdown(fd, 1) } }
