// 最小 TCP 中继参照(diagnosis-after-round-2 专用探针,非生产代码)。
// 目的:在与 rh-server 相同的机器/拓扑位置(test1)提供"最小仅 TCP 转发"
// 对照,区分 rh 实现开销与三机两跳路径的环境上限。
// 用法: refrelay <listen_port> <target_host:port> <copy|splice>
//   copy   —— 64KiB read/write 用户态拷贝循环(两方向各一线程)
//   splice —— pipe + splice(2) 零拷贝(SPLICE_F_MOVE,256KiB/次)
// 语义:半关闭透传(EOF -> shutdown 对端写侧);连接错误 -> 双向退出。
// 编译: rustc -O refrelay.rs -o refrelay
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::unix::io::AsRawFd;

extern "C" {
    fn pipe(fds: *mut i32) -> i32;
    fn close(fd: i32) -> i32;
    fn splice(
        fd_in: i32,
        off_in: *mut i64,
        fd_out: i32,
        off_out: *mut i64,
        len: usize,
        flags: u32,
    ) -> i64;
}

const SPLICE_F_MOVE: u32 = 1;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() != 4 {
        eprintln!("usage: {} <listen_port> <target_host:port> <copy|splice>", a[0]);
        std::process::exit(2);
    }
    let port: u16 = a[1].parse().expect("port");
    let target = a[2].clone();
    let mode = a[3].clone();
    assert!(mode == "copy" || mode == "splice", "mode 必须是 copy|splice");
    let ln = TcpListener::bind(("0.0.0.0", port)).expect("bind");
    println!("refrelay listening :{} -> {} mode={} pid={}", port, target, mode, std::process::id());
    for stream in ln.incoming() {
        let s = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("accept: {e}");
                continue;
            }
        };
        let t = target.clone();
        let m = mode.clone();
        std::thread::spawn(move || handle(s, t, m));
    }
}

fn handle(user: TcpStream, target: String, mode: String) {
    let upstream = match TcpStream::connect(&target) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("connect {target}: {e}");
            return;
        }
    };
    let u2 = user.try_clone().unwrap();
    let up2 = upstream.try_clone().unwrap();
    let m1 = mode.clone();
    let m2 = mode.clone();
    let h1 = std::thread::spawn(move || pump(&user, &upstream, &m1));
    let h2 = std::thread::spawn(move || pump(&up2, &u2, &m2));
    let _ = h1.join();
    let _ = h2.join();
}

fn pump(a: &TcpStream, b: &TcpStream, mode: &str) {
    match mode {
        "splice" => pump_splice(a, b),
        _ => pump_copy(a, b),
    }
}

fn pump_copy(a: &TcpStream, b: &TcpStream) {
    let mut a = a;
    let mut b = b;
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match a.read(&mut buf) {
            Ok(0) | Err(_) => {
                let _ = b.shutdown(Shutdown::Write);
                return;
            }
            Ok(n) => {
                if b.write_all(&buf[..n]).is_err() {
                    return;
                }
            }
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
        loop {
            let n = splice(a.as_raw_fd(), std::ptr::null_mut(), fds[1], std::ptr::null_mut(),
                           256 * 1024, SPLICE_F_MOVE);
            if n <= 0 {
                let _ = b.as_raw_fd();
                let _ = shutdown_wr(b.as_raw_fd());
                close(fds[0]);
                close(fds[1]);
                return;
            }
            let mut done: i64 = 0;
            while done < n {
                let m = splice(fds[0], std::ptr::null_mut(), b.as_raw_fd(), std::ptr::null_mut(),
                               (n - done) as usize, SPLICE_F_MOVE);
                if m <= 0 {
                    close(fds[0]);
                    close(fds[1]);
                    return;
                }
                done += m;
            }
        }
    }
}

extern "C" {
    fn shutdown(fd: i32, how: i32) -> i32;
}

fn shutdown_wr(fd: i32) -> i32 {
    unsafe { shutdown(fd, 1) } // SHUT_WR
}
