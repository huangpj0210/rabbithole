#!/usr/bin/env python3
"""rabbithole benchmark —— 目标服务(target 后端)。

在 rh-client 一侧运行,模拟被转发的内网服务。bench.sh 启动它,
rh-client 把隧道流量转发到这里。

提供三类后端(端口均可通过参数指定):
  --tcp-port   TCP 行协议服务(延迟 / 吞吐 / 并发)
  --udp-port   UDP 回显服务
  --http-port  HTTP 服务(vhost 基准)

TCP 行协议(首行命令,数据按命令含义跟随):
  PING            → PONG                                    单次往返,测延迟
  SEND <n>        → 发送 n 字节确定性数据后关闭              下行吞吐
  SEND <n> <mode> → mode: pattern(默认) / zeros / random    压缩对照
  RECV <n>        → 读取 n 字节并逐块校验,回 OK <n>         上行吞吐
  SLEEP <sec>     → 保持连接 sec 秒                          并发保持
  ECHO            → 原样回显直到对端关闭                      通用回显

数据确定性:pattern 模式为 1 MiB 周期序列,压测端按偏移逐块比对可检出损坏;
zeros 为全零(极易压缩),random 为确定性伪随机(不可压缩),用于压缩开关对照。
"""
from __future__ import annotations

import argparse
import http.server
import socket
import socketserver
import threading
import time

CHUNK = 1 << 20
PATTERN = bytes(range(256)) * (CHUNK // 256)
BACKLOG = 512


def pattern_slice(offset: int, n: int) -> bytes:
    """取确定性数据 [offset, offset+n) 片段(按 1 MiB 周期重复)。"""
    pos = offset % CHUNK
    if pos + n <= CHUNK:
        return PATTERN[pos:pos + n]
    out = bytearray()
    while len(out) < n:
        pos = (offset + len(out)) % CHUNK
        take = min(n - len(out), CHUNK - pos)
        out += PATTERN[pos:pos + take]
    return bytes(out)


def random_slice(offset: int, n: int) -> bytes:
    """确定性伪随机数据(不可压缩);以 offset 为种子,保证两端一致。"""
    import random
    rng = random.Random(offset // CHUNK)
    base = bytes(rng.getrandbits(8) for _ in range(CHUNK))
    pos = offset % CHUNK
    if pos + n <= CHUNK:
        return base[pos:pos + n]
    out = bytearray()
    while len(out) < n:
        p = (offset + len(out)) % CHUNK
        take = min(n - len(out), CHUNK - p)
        out += base[p:p + take]
    return bytes(out)


def payload_slice(mode: str, offset: int, n: int) -> bytes:
    if mode == "zeros":
        return bytes(n)
    if mode == "random":
        return random_slice(offset, n)
    return pattern_slice(offset, n)


def handle_tcp(conn: socket.socket) -> None:
    """单条 TCP 连接:循环处理命令直到对端关闭(持久连接)。

    每次 recv 一个命令并响应,支持同一连接上多轮 PING(SLEEP/ECHO 除外,
    它们改变连接语义后即结束本轮)。"""
    try:
        conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        f = conn.makefile("rb")
        while True:
            line = f.readline(256)
            if not line:
                return
            parts = line.strip().split()
            if not parts:
                continue
            op = parts[0].upper()

            if op == b"PING":
                conn.sendall(b"PONG\n")

            elif op == b"SEND":
                total = int(parts[1])
                mode = parts[2].decode() if len(parts) > 2 else "pattern"
                sent = 0
                while sent < total:
                    n = min(CHUNK, total - sent)
                    conn.sendall(payload_slice(mode, sent, n))
                    sent += n
                return

            elif op == b"RECV":
                total = int(parts[1])
                got = 0
                while got < total:
                    n = min(CHUNK, total - got)
                    buf = f.read(n)
                    if not buf or len(buf) != n:
                        return
                    if buf != pattern_slice(got, n):
                        conn.sendall(b"ERR corrupt\n")
                        return
                    got += n
                conn.sendall(b"OK %d\n" % total)

            elif op == b"SLEEP":
                time.sleep(float(parts[1]))
                return

            elif op == b"ECHO":
                while True:
                    d = conn.recv(65536)
                    if not d:
                        break
                    conn.sendall(d)
                return

            else:
                conn.sendall(b"ERR unknown\n")
    except (OSError, ValueError):
        pass
    finally:
        try:
            conn.close()
        except OSError:
            pass


class TcpTarget(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True
    request_queue_size = BACKLOG

    def handle_error(self, request, client_address):  # 连接级异常不打断服务
        pass


class TcpHandler(socketserver.BaseRequestHandler):
    def handle(self) -> None:
        handle_tcp(self.request)


def udp_worker(sock: socket.socket) -> None:
    """UDP 回显循环;多线程 + SO_REUSEPORT 提升吞吐。"""
    while True:
        try:
            data, addr = sock.recvfrom(65535)
        except OSError:
            return
        try:
            sock.sendto(data, addr)
        except OSError:
            pass


def start_udp(port: int, workers: int) -> list[socket.socket]:
    socks = []
    for _ in range(workers):
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        if hasattr(socket, "SO_REUSEPORT"):
            s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1)
        s.bind(("0.0.0.0", port))
        threading.Thread(target=udp_worker, args=(s,), daemon=True).start()
        socks.append(s)
    return socks


class HttpHandler(http.server.BaseHTTPRequestHandler):
    """固定响应体的 HTTP/1.1 服务,支持 keep-alive。"""

    protocol_version = "HTTP/1.1"
    body = b"x" * 4096

    def do_GET(self) -> None:  # noqa: N802 (stdlib 命名)
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(self.body)))
        self.end_headers()
        self.wfile.write(self.body)

    def log_message(self, *args) -> None:  # 静默,避免日志拖慢压测
        pass


class HttpTarget(http.server.ThreadingHTTPServer):
    allow_reuse_address = True
    daemon_threads = True
    request_queue_size = BACKLOG


def main() -> int:
    ap = argparse.ArgumentParser(description="rabbithole benchmark 目标服务")
    ap.add_argument("--tcp-port", type=int, default=0)
    ap.add_argument("--udp-port", type=int, default=0)
    ap.add_argument("--http-port", type=int, default=0)
    ap.add_argument("--udp-workers", type=int, default=4)
    ap.add_argument("--ready-file", default="", help="监听就绪后写入该文件(供编排等待)")
    args = ap.parse_args()

    started = []
    if args.tcp_port:
        srv = TcpTarget(("0.0.0.0", args.tcp_port), TcpHandler)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        started.append(f"tcp:{args.tcp_port}")
    if args.udp_port:
        start_udp(args.udp_port, args.udp_workers)
        started.append(f"udp:{args.udp_port}")
    if args.http_port:
        hsrv = HttpTarget(("0.0.0.0", args.http_port), HttpHandler)
        threading.Thread(target=hsrv.serve_forever, daemon=True).start()
        started.append(f"http:{args.http_port}")

    if not started:
        print("未指定任何监听端口", flush=True)
        return 2

    print("target ready " + " ".join(started), flush=True)
    if args.ready_file:
        with open(args.ready_file, "w") as fh:
            fh.write(" ".join(started))

    try:
        while True:
            time.sleep(3600)
    except KeyboardInterrupt:
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
