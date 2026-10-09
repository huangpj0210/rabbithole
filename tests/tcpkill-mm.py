#!/usr/bin/env python3
"""rabbithole E2E 中间人:可控 TCP 代理,用于"仅数据通道断开/迟到连接"故障注入。

用法: tcpkill-mm.py <listen_port> <upstream_host> <upstream_port> <cmd_dir>

约定:
- 首条建立的连接 = 控制会话(绝不干扰);后续连接 = 数据通道。
- 命令(以文件出现为信号,处理后改名为 <cmd>.done):
  - kill_data   关闭全部数据通道连接(双向关闭),控制连接保持
  - hold        此后新建数据通道的"客户端→服务端"首包被扣留(迟到连接)
  - release     放行全部被扣留首包
"""
import os
import socket
import sys
import threading
import time

RELEASE_DONE = None


def pump(a, b):
    try:
        while True:
            d = a.recv(65536)
            if not d:
                break
            b.sendall(d)
    except OSError:
        pass
    finally:
        try:
            b.shutdown(socket.SHUT_WR)
        except OSError:
            pass


def released(cmd_dir):
    return os.path.exists(os.path.join(cmd_dir, "release.done"))


def main():
    listen_port, up_host, up_port, cmd_dir = (
        int(sys.argv[1]),
        sys.argv[2],
        int(sys.argv[3]),
        sys.argv[4],
    )
    os.makedirs(cmd_dir, exist_ok=True)
    # 清理上一轮命令残留
    for f in ("kill_data", "hold", "release", "kill_data.done", "hold.done", "release.done"):
        p = os.path.join(cmd_dir, f)
        if os.path.exists(p):
            os.remove(p)

    state = {"control_up": None, "hold": False}
    lock = threading.Lock()
    data_conns = []

    def hold_first_then_pump(down, up, cmd_dir):
        # 扣留客户端→服务端首包(模拟迟到连接),release 后放行并正常转发
        first = down.recv(65536)
        while not released(cmd_dir):
            time.sleep(0.05)
        try:
            up.sendall(first)
        except OSError:
            return
        pump(down, up)

    def handle(down):
        up = socket.socket()
        up.connect((up_host, up_port))
        with lock:
            is_control = state["control_up"] is None
            holding = state["hold"]
            if is_control:
                state["control_up"] = up
            else:
                data_conns.append((down, up))
        if not is_control and holding:
            threading.Thread(
                target=hold_first_then_pump, args=(down, up, cmd_dir), daemon=True
            ).start()
        else:
            threading.Thread(target=pump, args=(down, up), daemon=True).start()
        threading.Thread(target=pump, args=(up, down), daemon=True).start()

    def acceptor():
        srv = socket.socket()
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(("127.0.0.1", listen_port))
        srv.listen(64)
        while True:
            down, _ = srv.accept()
            threading.Thread(target=handle, args=(down,), daemon=True).start()

    threading.Thread(target=acceptor, daemon=True).start()

    while True:
        for cmd in ("kill_data", "hold", "release"):
            f = os.path.join(cmd_dir, cmd)
            if os.path.exists(f):
                if cmd == "kill_data":
                    with lock:
                        pairs = list(data_conns)
                        data_conns.clear()
                    for d, u in pairs:
                        for s in (d, u):
                            try:
                                s.close()
                            except OSError:
                                pass
                elif cmd == "hold":
                    with lock:
                        state["hold"] = True
                elif cmd == "release":
                    open(os.path.join(cmd_dir, "release.done"), "w").close()
                os.rename(f, f + ".done")
        time.sleep(0.05)


if __name__ == "__main__":
    main()
