#!/usr/bin/env python3
"""rabbithole XTCP 隔离网络验证:用户态 NAT 网关(STUN + UDP 中继)。

用法: natgw.py <stun_port> <mode> <stats_file>   # mode: open | block

行为:
- STUN(RFC 5389 Binding)监听 stun_port。内部主机(打洞 socket)发来 Binding
  Request 时,分配一个"公网"中继 socket E,以 XOR-MAPPED-ADDRESS 返回 E。
- 中继:E 上收到的对端报文转发给内部主机;内部主机发往 E 的报文转发给对端。
  双向源地址均呈现为 E(与真实 NAT 一致:对端只见映射地址,不见内部原始地址)。
- open 模式正常转发;block 模式丢弃对端方向报文(模拟打洞被 NAT 拦截)。
- stats_file 周期写入 JSON:{relayed_bytes, packets_in, packets_blocked},
  供测试断言数据确实经过网关/确实被拦截。
"""
import json
import socket
import sys
import threading
import time

MAGIC = 0x2112A442


def xor_mapped(tx_id, ip, port):
    # body: 0x00 family(1B) xport(2B) xaddr(4B)
    xport = port ^ (MAGIC >> 16)
    cookie = MAGIC.to_bytes(4, "big")
    xaddr = bytes(a ^ b for a, b in zip(ip, cookie))
    body = b"\x00\x01" + xport.to_bytes(2, "big") + xaddr
    return (0x0020).to_bytes(2, "big") + len(body).to_bytes(2, "big") + body


def main():
    stun_port, mode, stats_file = int(sys.argv[1]), sys.argv[2], sys.argv[3]
    stats = {"relayed_bytes": 0, "packets_in": 0, "packets_blocked": 0, "ext_port": 0}

    stun = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    stun.bind(("127.0.0.1", stun_port))

    mappings = {}  # internal addr -> ext socket
    peers = {}  # ext port -> last peer addr
    lock = threading.Lock()

    def ext_loop(ext, internal):
        while True:
            try:
                data, src = ext.recvfrom(65536)
            except OSError:
                return
            with lock:
                stats["packets_in"] += 1
                if src == internal:
                    peer = peers.get(ext.getsockname()[1])
                    if peer and mode == "open":
                        stats["relayed_bytes"] += len(data)
                        ext.sendto(data, peer)
                else:
                    peers[ext.getsockname()[1]] = src
                    if mode == "open":
                        stats["relayed_bytes"] += len(data)
                        ext.sendto(data, internal)
                    else:
                        stats["packets_blocked"] += 1

    def stats_loop():
        while True:
            with open(stats_file, "w") as f:
                json.dump(stats, f)
            time.sleep(0.3)

    threading.Thread(target=stats_loop, daemon=True).start()

    while True:
        data, src = stun.recvfrom(2048)
        if len(data) < 20:
            continue
        tx_id = data[8:20]
        with lock:
            ext = mappings.get(src)
            if ext is None:
                ext = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                ext.bind(("127.0.0.1", 0))
                mappings[src] = ext
                stats["ext_port"] = ext.getsockname()[1]
                threading.Thread(
                    target=ext_loop, args=(ext, src), daemon=True
                ).start()
        ip = socket.inet_aton("127.0.0.1")
        port = ext.getsockname()[1]
        attr = xor_mapped(tx_id, ip, port)
        resp = (
            (0x0101).to_bytes(2, "big")
            + len(attr).to_bytes(2, "big")
            + MAGIC.to_bytes(4, "big")
            + tx_id
            + attr
        )
        stun.sendto(resp, src)


if __name__ == "__main__":
    main()
