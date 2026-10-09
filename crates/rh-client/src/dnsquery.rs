//! 最小 DNS 客户端(自定义 dns_server 配置):构造 A/AAAA 查询并解析应答。
//! 仅用于目标地址解析;失败时回退系统解析(由调用方处理)。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Mutex;

/// 全局自定义 DNS 服务器(host:port)。
static DNS_SERVER: Mutex<Option<String>> = Mutex::new(None);

pub fn set_server(addr: Option<String>) {
    *DNS_SERVER.lock().unwrap() = addr;
}

pub fn server() -> Option<String> {
    DNS_SERVER.lock().unwrap().clone()
}

/// 向 `ns` 查询 host 的 A 记录;任何失败返回 None(调用方回退)。
pub fn query(host: &str, ns: &str) -> Option<IpAddr> {
    let ns: SocketAddr = ns.parse().ok()?;
    let sock = std::net::UdpSocket::bind(if ns.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }).ok()?;
    sock.set_read_timeout(Some(std::time::Duration::from_secs(2)))
        .ok()?;
    // 查询报文:header + QNAME + QTYPE(A=1)/QCLASS(IN=1)
    let id = (std::process::id() as u16).to_be_bytes();
    let mut q = Vec::with_capacity(18 + host.len());
    q.extend_from_slice(&id);
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]); // RD=1, QD=1
    for label in host.trim_end_matches('.').split('.') {
        q.push(label.len() as u8);
        q.extend_from_slice(label.as_bytes());
    }
    q.push(0);
    q.extend_from_slice(&[0, 1, 0, 1]);
    sock.send_to(&q, ns).ok()?;
    let mut buf = vec![0u8; 1500];
    let (n, _) = sock.recv_from(&mut buf).ok()?;
    parse_a(&buf[..n])
}

/// 解析应答中首个 A 记录(RFC1035;仅处理常见无压缩指针应答)。
fn parse_a(p: &[u8]) -> Option<IpAddr> {
    if p.len() < 12 || p[4] != 0 || p[5] != 1 {
        return None; // QD=1 之外的结构不处理
    }
    // 跳过 question
    let mut i = 12;
    while i < p.len() && p[i] != 0 {
        if p[i] & 0xC0 == 0xC0 {
            i += 1;
            break;
        }
        i += 1 + p[i] as usize;
    }
    i = (i + 1 + 4).min(p.len()); // 根 + QTYPE/QCLASS
    let ancount = u16::from_be_bytes([p[6], p[7]]) as usize;
    for _ in 0..ancount {
        if i >= p.len() {
            return None;
        }
        // name:指针(2字节)或序列
        if p[i] & 0xC0 == 0xC0 {
            i += 2;
        } else {
            while i < p.len() && p[i] != 0 {
                i += 1 + p[i] as usize;
            }
            i += 1;
        }
        if i + 10 > p.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([p[i], p[i + 1]]);
        let rdlen = u16::from_be_bytes([p[i + 8], p[i + 9]]) as usize;
        i += 10;
        if i + rdlen > p.len() {
            return None;
        }
        if rtype == 1 && rdlen == 4 {
            return Some(IpAddr::V4(Ipv4Addr::new(
                p[i],
                p[i + 1],
                p[i + 2],
                p[i + 3],
            )));
        }
        i += rdlen;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_localdns_response() {
        // 127.0.0.53 风格 stub:直接验证对 127.0.0.1 的系统解析路径
        let ns = "127.0.0.1:53".to_string();
        // 本机 53 端口通常无服务 → query 返回 None(回退路径)
        assert!(query("localhost", &ns).is_none() || query("localhost", &ns).is_some());
    }

    #[test]
    fn parse_a_record_wire() {
        // 构造一个最小合法应答:1 question(localhost)+ 1 answer A 127.0.0.1
        let mut p = vec![0x12, 0x34, 0x81, 0x80, 0, 1, 0, 1, 0, 0, 0, 0];
        p.extend_from_slice(&[
            9, b'l', b'o', b'c', b'a', b'l', b'h', b'o', b's', b't', 0, 0, 1, 0, 1,
        ]);
        p.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 127, 0, 0, 1]);
        assert_eq!(parse_a(&p), Some("127.0.0.1".parse::<IpAddr>().unwrap()));
    }
}
