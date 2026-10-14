#!/usr/bin/env python3
"""rabbithole benchmark —— 端点 /proc CPU+RSS 采样器(goal-repair §1.1 / §1.2)。

在 rh-server(test1)与 rh-client(test2)本机运行,对被测进程做**仅依赖
/proc** 的采样:
  * 频率 ≥10Hz(默认 20Hz,--hz);
  * 每条记录 (epoch_us, utime+stime ticks, rss_kb) —— 进程级,跨线程求和;
  * 首行写头 {clk_tck, hz, pid}(verify_goal 计算 CPU%/覆盖率要用);
  * 进程退出或 --max-hours 到期即停止。

CPU% 口径(docs/performance-repair-acceptance.md §1.1):
    Δticks / CLK_TCK / 窗口秒 × 100,100% = 一个满核。
覆盖率 = 窗口内实际样本数 / (窗口秒 × hz),< 95% 的样本判 invalid。

仅 Linux(有 /proc);其他平台启动即失败并把原因写进 stderr
(证据必须 fail closed,不许静默缺失)。
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time


def read_proc_stat(pid: int) -> tuple[int, int] | None:
    """返回 (utime+stime ticks, rss_kb);进程消失返回 None。"""
    try:
        with open(f"/proc/{pid}/stat", "rb") as fh:
            raw = fh.read()
    except OSError:
        return None
    # comm 可能含空格/括号:从最后一个 ')' 之后解析
    rp = raw.rfind(b")")
    if rp < 0:
        return None
    fields = raw[rp + 2:].split()
    # fields[0] = state;utime 是第 14 字段、stime 第 15 字段(从 state 起数 11/12)
    try:
        utime = int(fields[11])
        stime = int(fields[12])
    except (IndexError, ValueError):
        return None
    rss_kb = 0
    try:
        with open(f"/proc/{pid}/status", "rb") as fh:
            for line in fh:
                if line.startswith(b"VmRSS:"):
                    rss_kb = int(line.split()[1])
                    break
    except (OSError, ValueError, IndexError):
        pass
    return utime + stime, rss_kb


def main() -> int:
    ap = argparse.ArgumentParser(description="rh 进程 /proc CPU+RSS 采样器")
    ap.add_argument("--pid", type=int, required=True)
    ap.add_argument("--out", required=True, help="输出 JSONL 路径")
    ap.add_argument("--hz", type=float, default=20.0, help="采样频率(≥10)")
    ap.add_argument("--max-hours", type=float, default=12.0)
    args = ap.parse_args()

    if args.hz < 10:
        print(f"采样频率必须 ≥10Hz(得到 {args.hz})", file=sys.stderr)
        return 2
    if not os.path.isdir("/proc"):
        print("本平台无 /proc,CPU 证据只能产于 Linux 验收机(fail closed)", file=sys.stderr)
        return 3
    try:
        clk_tck = os.sysconf("SC_CLK_TCK")
    except (ValueError, OSError, AttributeError):
        clk_tck = 100

    interval = 1.0 / args.hz
    deadline = time.time() + args.max_hours * 3600
    n = 0
    with open(args.out, "w", encoding="utf-8") as fh:
        fh.write(json.dumps({"clk_tck": clk_tck, "hz": args.hz, "pid": args.pid}) + "\n")
        fh.flush()
        while time.time() < deadline:
            got = read_proc_stat(args.pid)
            if got is None:
                # 被测进程退出:如实收尾,不视为采样器失败
                fh.write(json.dumps({"t_us": int(time.time() * 1e6), "ticks": None,
                                     "rss_kb": None, "process_gone": True}) + "\n")
                fh.flush()
                print(f"pid {args.pid} 已退出,采样器收尾(样本 {n} 条)", file=sys.stderr)
                return 0
            ticks, rss = got
            fh.write(json.dumps({"t_us": int(time.time() * 1e6),
                                 "ticks": ticks, "rss_kb": rss}) + "\n")
            n += 1
            if n % int(args.hz * 10) == 0:
                fh.flush()
            time.sleep(interval)
    print(f"达到 max-hours 上限,采样器收尾(样本 {n} 条)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
