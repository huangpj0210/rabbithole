#!/usr/bin/env python3
"""rabbithole benchmark —— 结果汇总。

读取 runs/ 下的场景 JSON,生成:
  * summary.md   人类可读汇总(表格 + 关键结论)
  * summary.json 机器可读(供 CI 对比 / 回归检测)

用法:
  summarize.py <runs_dir> [--out <dir>]
  summarize.py <runs_dir> --gates        # goal-repair 门槛判定(经 verify_goal)

对齐 RFC 2544/6349 的呈现惯例:吞吐给绝对值 + 与直连基线的比值,
延迟给百分位(而非均值),并显式标注错误/丢包。

★goal-repair A10 修复:peer 汇总的键包含 label(实验标识),同键多样本
给 中位数/最小/最大,失败样本显式呈现 —— 旧版同 proto 键位后写覆盖先写,
peer-f2 目录 8 个 jsonl 同为 tcp 时只有末文件的值存活。
单流门槛只从 (path=*, streams=1) 专属计算,不以多流峰值替代。
"""
from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path


def load(path: Path):
    try:
        return json.loads(path.read_text())
    except (OSError, json.JSONDecodeError):
        return None


def median(vals):
    """中位数(G7:每项数字 = ≥3 次独立配对测量的中位数)。"""
    if not vals:
        return None
    s = sorted(vals)
    mid = len(s) // 2
    if len(s) % 2:
        return s[mid]
    return (s[mid - 1] + s[mid]) / 2


def stats(vals):
    if not vals:
        return {"n": 0}
    return {"n": len(vals), "median": median(vals), "min": min(vals), "max": max(vals)}


def fmt(v, unit="", nd=2):
    if v is None:
        return "—"
    if isinstance(v, float):
        return f"{round(v, nd)}{unit}"
    return f"{v}{unit}"


def ratio(num, den):
    if not num or not den or den == 0:
        return None
    return num / den


def section_latency(d: dict, out: list) -> None:
    base = d.get("latency_baseline")
    tun = d.get("latency_tunnel")
    nc = d.get("latency_newconn")
    if not (base or tun):
        return
    out.append("## 延迟(隧道内往返,毫秒)\n")
    out.append("| 路径 | p50 | p90 | p99 | p99.9 | max | 抖动(σ) |")
    out.append("|---|---|---|---|---|---|---|")
    for name, r in (("直连基线", base), ("经隧道", tun), ("新建连接", nc)):
        if not r:
            continue
        out.append(
            f"| {name} | {fmt(r.get('p50_ms'))} | {fmt(r.get('p90_ms'))} | "
            f"{fmt(r.get('p99_ms'))} | {fmt(r.get('p999_ms'))} | {fmt(r.get('max_ms'))} | "
            f"{fmt(r.get('stddev_ms'))} |"
        )
    if base and tun:
        dp50 = (tun.get("p50_ms") or 0) - (base.get("p50_ms") or 0)
        dp99 = (tun.get("p99_ms") or 0) - (base.get("p99_ms") or 0)
        out.append(f"\n**隧道开销**:p50 +{fmt(dp50)}ms,p99 +{fmt(dp99)}ms\n")


def section_throughput(d: dict, out: list) -> None:
    rows = [(k, v) for k, v in d.items() if k.startswith("throughput_")]
    if not rows:
        return
    out.append("## 吞吐(iperf3)\n")
    out.append("| 配置 | 方向 | Mbps | 重传 |")
    out.append("|---|---|---|---|")
    for k, r in sorted(rows):
        label = k.replace("throughput_", "")
        if isinstance(r, dict) and "end" in r:
            recv = r["end"].get("sum_received") or r["end"].get("sum") or {}
            sent = r["end"].get("sum_sent") or {}
            mbps = (recv.get("bits_per_second") or sent.get("bits_per_second") or 0) / 1e6
            retr = sent.get("retransmits", "—")
            out.append(f"| {label} | — | {fmt(mbps)} | {retr} |")
        else:
            out.append(f"| {label} | {fmt(r.get('mbps') if isinstance(r, dict) else None)} | | |")


def section_peer(d: dict, out: list) -> None:
    peer = d.get("peer")
    if not peer:
        return
    out.append("## 跨机吞吐(隧道 vs 直连)\n")
    out.append("| 模式 | 流数 | Mbps | 相对直连 |")
    out.append("|---|---|---|---|")
    direct = {r["streams"]: r["mbps"] for r in peer if r.get("mode") == "direct"}
    for r in peer:
        base = direct.get(r.get("streams"))
        rel = f"{round(r['mbps']/base*100,1)}%" if base and r.get("mode") == "tunnel" else "—"
        out.append(f"| {r.get('mode')} | {r.get('streams')} | {fmt(r.get('mbps'))} | {rel} |")


def section_concurrent(d: dict, out: list) -> None:
    rows = [(k, v) for k, v in d.items()
            if k.startswith("concurrent_") and not k.startswith("concurrent_direct_")]
    if not rows:
        return
    out.append("## 并发连接(直连 vs 隧道,同一压测端)\n")
    out.append("| 并发数 | 直连成功 | 隧道成功 | 隧道失败 | 建连 p50 | 建连 p99 |")
    out.append("|---|---|---|---|---|---|")
    for k, r in sorted(rows, key=lambda x: int(x[0].split("_")[1])):
        n = k.split("_")[1]
        direct = d.get(f"concurrent_direct_{n}") or {}
        out.append(
            f"| {n} | {direct.get('established', '—')} | {r.get('established')} | "
            f"{r.get('failed')} | {fmt(r.get('connect_p50_ms'))} | {fmt(r.get('connect_p99_ms'))} |"
        )
    out.append("\n> 直连与隧道在同一轮同并发下对比;两者接近说明瓶颈在测试装置,"
               "隧道显著更低才说明隧道自身受限。\n")


def section_udp(d: dict, out: list) -> None:
    rows = [(k, v) for k, v in d.items() if k.startswith("udp_")]
    if not rows:
        return
    out.append("## UDP 转发\n")
    out.append("| 报文大小 | 接收 | 丢包% | Mbps | pps | RTT p50 |")
    out.append("|---|---|---|---|---|---|")
    for k, r in sorted(rows, key=lambda x: int(x[0].split("_")[1])):
        size = k.split("_")[1]
        if r.get("error"):
            out.append(f"| {size}B | ✗ {r['error']} | — | — | — | — |")
        else:
            out.append(
                f"| {size}B | {r.get('received')} | {fmt(r.get('loss_pct'))} | "
                f"{fmt(r.get('mbps'))} | {fmt(r.get('pps'), nd=0)} | {fmt(r.get('rtt_p50_ms'))} |"
            )


def section_http(d: dict, out: list) -> None:
    base = d.get("http_baseline")
    tun = d.get("http_tunnel")
    if not (base or tun):
        return
    out.append("## vhost HTTP\n")
    out.append("| 路径 | QPS | 请求数 | 延迟 p50 | 延迟 p99 |")
    out.append("|---|---|---|---|---|")
    for name, r in (("直连", base), ("经隧道", tun)):
        if r:
            out.append(
                f"| {name} | {fmt(r.get('qps'))} | {r.get('requests')} | "
                f"{fmt(r.get('lat_p50_ms'))} | {fmt(r.get('lat_p99_ms'))} |"
            )
    if base and tun:
        rel = ratio(tun.get("qps"), base.get("qps"))
        if rel:
            out.append(f"\n**隧道 QPS 保留率**: {round(rel*100,1)}%\n")


def section_soak(d: dict, out: list) -> None:
    r = d.get("soak")
    if not r:
        return
    out.append("## 持续压测(衰减检测)\n")
    buckets = r.get("buckets") or []
    rps = [b["rps"] for b in buckets] or [0]
    head = sum(rps[:10]) / max(len(rps[:10]), 1)
    tail = sum(rps[-10:]) / max(len(rps[-10:]), 1)
    drift = ((tail - head) / head * 100) if head else 0
    out.append(f"- 时长 {r.get('duration')}s,并发 {r.get('connections')},"
               f"总往返 {r.get('total_rounds')},错误 {r.get('errors')}")
    out.append(f"- RPS:均值 {fmt(r.get('rps_mean'), nd=1)},"
               f"min {r.get('rps_min')},max {r.get('rps_max')}")
    out.append(f"- 首段→末段漂移:**{round(drift,2)}%**"
               f"({'稳定' if abs(drift) < 10 else '注意:存在衰减/抖动'})\n")


def section_misc(d: dict, out: list) -> None:
    # 传输对照
    rows = [(k, v) for k, v in d.items() if k.startswith("transport_")]
    if rows:
        out.append("## 传输协议对照\n")
        out.append("| 协议 | 状态 | p50 | p99 | 并发成功/256 |")
        out.append("|---|---|---|---|---|")
        for k, r in sorted(rows):
            proto = k.replace("transport_", "")
            if not r.get("ok"):
                out.append(f"| {proto} | ✗ 失败 | — | — | — |")
            else:
                est = r.get("conn_established") or 0
                fail = r.get("conn_failed") or 0
                out.append(
                    f"| {proto} | ✓ | {fmt(r.get('lat_p50_ms'))} | {fmt(r.get('lat_p99_ms'))} | "
                    f"{est}/{est + fail} |"
                )
        out.append("")

    # 池
    rows = [(k, v) for k, v in d.items() if k.startswith("pool_")]
    if rows:
        out.append("## 预热池大小 vs 新建连接延迟\n")
        out.append("| pool_size | p50 | p99 |")
        out.append("|---|---|---|")
        for k, r in sorted(rows, key=lambda x: int(x[0].split("_")[1])):
            out.append(f"| {k.split('_')[1]} | {fmt(r.get('p50_ms'))} | {fmt(r.get('p99_ms'))} |")
        out.append("")

    # 压缩
    rows = [(k, v) for k, v in d.items() if k.startswith("compress_")]
    if rows:
        out.append("## 压缩开关对照\n")
        out.append("| compression | 载荷 | Mbps |")
        out.append("|---|---|---|")
        for k, r in sorted(rows):
            _, flag, mode = k.split("_")
            out.append(f"| {flag} | {mode} | {fmt(r.get('mbps'))} |")
        out.append("")

    # 限速
    free = d.get("ratelimit_free")
    cap = d.get("ratelimit_50MB")
    if free or cap:
        out.append("## 带宽限制\n")
        out.append(f"- 无限速: {fmt(free.get('mbps') if free else None)} Mbps")
        out.append(f"- 限 50MB/s(400Mbps): {fmt(cap.get('mbps') if cap else None)} Mbps\n")

    # 资源
    for k in ("resource_idle_server", "resource_load_server", "resource_load_client"):
        r = d.get(k)
        if r:
            out.append(f"- `{k}`: CPU {r.get('cpu_pct')}%,RSS {r.get('rss_kb')} KB")
    if any(d.get(k) for k in ("resource_idle_server", "resource_load_server")):
        out.append("")

    # 冒烟
    sm = d.get("smoke")
    if sm:
        out.append(f"## 冒烟\n- 通过 {sm.get('pass')},失败 {sm.get('fail')}\n")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("runs_dir")
    ap.add_argument("--out", default="")
    ap.add_argument("--gates", action="store_true",
                    help="goal-repair 门槛判定(G1-G7;经 py/verify_goal.py,"
                         "数据不足=fail;任何门槛未过退出码非 0)")
    args = ap.parse_args()

    rd = Path(args.runs_dir)
    if not rd.is_dir():
        print(f"不是目录: {rd}", file=sys.stderr)
        return 1

    if args.gates:
        return run_gates(rd)

    # peer 三机结果目录(results-<proto>.jsonl)
    jsonl = sorted(rd.glob("results-*.jsonl"))
    if jsonl:
        return summarize_peer(rd, jsonl, Path(args.out) if args.out else rd)

    data = {}
    for f in sorted(rd.glob("*.json")):
        v = load(f)
        if v is not None:
            data[f.stem] = v

    out: list[str] = []
    out.append(f"# rabbithole benchmark 汇总\n")
    out.append(f"- 结果目录:`{rd}`")
    out.append(f"- 场景文件数:{len(data)}")
    out.append(f"- 基准版本:{os.environ.get('RH_VERSION', 'v0.3.0')}\n")

    section_misc(data, out)
    section_latency(data, out)
    section_throughput(data, out)
    section_peer(data, out)
    section_concurrent(data, out)
    section_udp(data, out)
    section_http(data, out)
    section_soak(data, out)

    md = "\n".join(out) + "\n"
    outdir = Path(args.out) if args.out else rd
    outdir.mkdir(parents=True, exist_ok=True)
    (outdir / "summary.md").write_text(md)
    (outdir / "summary.json").write_text(json.dumps(data, indent=2, ensure_ascii=False))

    print(md)
    print(f"\n已写入 {outdir/'summary.md'} 与 {outdir/'summary.json'}", file=sys.stderr)
    return 0



def run_gates(rd: Path) -> int:
    """门槛判定统一走 verify_goal(单一实现,避免两处口径漂移)。"""
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import verify_goal  # noqa: E402 (同目录导入)

    ok, report = verify_goal.verify_path(str(rd))
    print(report)
    return 0 if ok else 1


# ---------- peer 三机结果汇总(results-*.jsonl) ----------
# ★A10 修复:键含 label(实验标识);同键多样本给中位数/最小/最大;
# 失败/无效样本显式呈现;单流门槛只从 streams=1 计算,不以多流峰值替代。

def summarize_peer(rd: Path, jsonl, outdir: Path) -> int:
    recs: list[dict] = []
    bad_lines: list[dict] = []
    for f in jsonl:
        for no, line in enumerate(f.read_text(encoding="utf-8").splitlines(), 1):
            line = line.strip()
            if not line:
                continue
            try:
                recs.append(json.loads(line))
            except json.JSONDecodeError:
                # 旧版会"修复"空吞吐行(`"mbps":,`)后照常汇总 —— 现在如实列为坏行
                bad_lines.append({"file": f.name, "line": no})

    th: dict = {}          # (label,proto,mux,relay,path,dir,streams) -> [样本]
    th_pairs: dict = {}    # (label,proto,mux,relay,dir,streams,sample) -> {path: mbps}
    conc: dict = {}        # (label,proto,mux,relay,path,n) -> [样本 dict]
    udp: dict = {}         # (label,proto,mux,relay,path) -> [样本 dict]
    metas: list[dict] = []
    invalid: list[dict] = []

    for r in recs:
        metric = r.get("metric")
        label = r.get("label", "?")
        proto = r.get("proto", "?")
        mux = r.get("mux", "?")
        relay = r.get("relay", "?")
        if metric == "meta":
            metas.append(r)
            if r.get("env_error"):
                invalid.append({"kind": "meta_env_error", "rec": r})
            continue
        if metric == "throughput":
            valid = r.get("valid") is True and isinstance(r.get("mbps"), (int, float))
            key = (label, proto, mux, relay, r.get("path"), r.get("dir"), r.get("streams"))
            if valid:
                th.setdefault(key, []).append(r["mbps"])
                pk = (label, proto, mux, relay, r.get("dir"), r.get("streams"), r.get("sample"))
                th_pairs.setdefault(pk, {})[r.get("path")] = r["mbps"]
            else:
                invalid.append({"kind": "throughput_invalid", "rec": r})
        elif metric == "concurrent":
            key = (label, proto, mux, relay, r.get("path"), r.get("n"))
            conc.setdefault(key, []).append(r)
            if r.get("ok") is False or "env_error" in r:
                invalid.append({"kind": "concurrent_invalid", "rec": r})
        elif metric == "udp":
            d = r.get("data", r)
            ok = not (r.get("ok") is False or d.get("ok") is False or "error" in d
                      or "env_error" in d)
            udp.setdefault((label, proto, mux, relay, r.get("path")), []).append(
                d if ok else {"_invalid": d})
            if not ok:
                invalid.append({"kind": "udp_invalid", "rec": r})

    out: list[str] = []
    out.append("# rabbithole 跨机基准汇总(按实验分组)\n")
    out.append(f"- 结果目录:`{rd}`;记录 {len(recs)} 条,坏行 {len(bad_lines)} 条"
               + (f"(位置:{bad_lines})" if bad_lines else ""))
    labels = sorted({str(k[0]) for k in list(th) + list(conc) + list(udp)}
                    | {str(m.get("label")) for m in metas if m.get("label")})
    out.append(f"- 实验(标签):{', '.join(labels)}\n")

    # ---- 吞吐:中位数 + 配对保留率 ----
    out.append("## 吞吐(中位数 / 最小 / 最大;直连 vs 隧道;保留率=配对比值的中位数)\n")
    out.append("| 标签 | 协议 | mux | relay | 方向 | 流数 | 直连 Mbps(中/最小/最大,n) | 隧道 Mbps(中/最小/最大,n) | 配对保留率中位数 |")
    out.append("|---|---|---|---|---|---:|---|---|---:|")

    def cell(vals):
        s = stats(vals)
        return (f"{round(s['median'],1)} / {round(s['min'],1)} / {round(s['max'],1)}"
                f" (n={s['n']})")

    for key in sorted(th, key=lambda k: (str(k[0]), str(k[1]), str(k[4]), str(k[5]), k[6])):
        label, proto, mux, relay, path, direction, streams = key
        if path != "direct":
            continue
        tkey = (label, proto, mux, relay, "tunnel", direction, streams)
        if tkey not in th:
            continue
        ds, ts = th[key], th[tkey]
        ratios = []
        pk_base = key[:4] + (direction, streams)
        for pk, pv in th_pairs.items():
            if pk[:6] != pk_base:
                continue
            if pv.get("direct", 0) and pv.get("tunnel") is not None:
                ratios.append(pv["tunnel"] / pv["direct"])
        ratio_txt = f"{round(median(ratios)*100,1)}%" if ratios else "配对不足"
        out.append(f"| {label} | {proto} | {mux} | {relay} | {direction} | {streams} "
                   f"| {cell(ds)} | {cell(ts)} | {ratio_txt} |")

    # ---- 并发:verified 口径(A12)----
    out.append("\n## 并发连接(established / **verified=应用层往返+保持存活**)\n")
    out.append("| 标签 | 协议 | mux | 并发 | 路径 | established(中位) | verified(中位) | hold_alive(中位) | 失败(总) |")
    out.append("|---|---|---|---:|---|---:|---:|---:|---:|")
    for key in sorted(conc, key=lambda k: (str(k[0]), str(k[1]), k[5] or 0, str(k[4]))):
        label, proto, mux, relay, path, n = key
        samples = conc[key]
        est = median([s.get("established", 0) for s in samples])
        ver = median([s.get("verified", 0) for s in samples])
        alive = median([s.get("hold_alive", 0) for s in samples])
        failed = sum(s.get("failed", 0) for s in samples)
        out.append(f"| {label} | {proto} | {mux} | {n} | {path} "
                   f"| {est} | {ver} | {alive} | {failed} |")

    # ---- UDP(排空后口径)----
    out.append("\n## UDP(损失=排空后去重口径;late=排空期到达)\n")
    out.append("| 标签 | 协议 | 路径 | 有效样本 | sent | recv_unique | late_recv | dup | 损失% (中位) |")
    out.append("|---|---|---|---:|---:|---:|---:|---:|---:|")
    for key in sorted(udp, key=lambda k: (str(k[0]), str(k[1]), str(k[4]))):
        label, proto, mux, relay, path = key
        samples = [s for s in udp[key] if "_invalid" not in s]
        n_inv = len(udp[key]) - len(samples)
        if not samples:
            out.append(f"| {label} | {proto} | {path} | 全部无效({n_inv}) | — | — | — | — | — |")
            continue
        loss = median([s.get("loss_pct", 100) for s in samples])
        out.append(
            f"| {label} | {proto} | {path} | {len(samples)}"
            + (f"(+{n_inv} 无效)" if n_inv else "")
            + f" | {sum(s.get('sent', 0) for s in samples)} "
            f"| {sum(s.get('recv_unique', 0) for s in samples)} "
            f"| {sum(s.get('late_recv', 0) for s in samples)} "
            f"| {sum(s.get('dup', 0) for s in samples)} | {round(loss,3)} |")

    # ---- 无效样本(显式呈现,绝不静默丢弃)----
    out.append(f"\n## 无效样本 / 失败({len(invalid)} 条)\n")
    if invalid:
        for item in invalid[:50]:
            r = item["rec"]
            d = r.get("data", {})
            out.append(
                f"- [{item['kind']}] label={r.get('label')} metric={r.get('metric')} "
                f"path={r.get('path')} dir={r.get('dir')} streams={r.get('streams')} "
                f"sample={r.get('sample')} exit={r.get('exit')} "
                f"err={json.dumps(r.get('error') or r.get('env_error') or d.get('error') or d.get('env_error') or '', ensure_ascii=False)}")
        if len(invalid) > 50:
            out.append(f"- …另有 {len(invalid)-50} 条(见 results 文件)")
    else:
        out.append("- 无")

    md = "\n".join(out) + "\n"
    outdir.mkdir(parents=True, exist_ok=True)
    (outdir / "summary.md").write_text(md)
    (outdir / "summary.json").write_text(
        json.dumps({"records": recs, "bad_lines": bad_lines, "invalid": invalid},
                   indent=2, ensure_ascii=False))
    print(md)
    print(f"\n已写入 {outdir / 'summary.md'}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
