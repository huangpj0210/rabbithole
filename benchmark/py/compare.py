#!/usr/bin/env python3
"""rabbithole benchmark —— rabbithole vs frp 三方对比。

用法:
  compare.py <rh结果目录> <frp结果目录> [--out <dir>]

两个目录都是 results-<proto>.jsonl 的集合(记录格式相同)。
输出COMPARE.md:同一指标下 直连 / rabbithole / frp / RH-frp 比值 并排。
判读原则:双方共享同一基线;保留率(隧道/直连)看实现效率,RH/frp 看相对差距。
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def load_dir(rd: Path) -> list[dict]:
    recs = []
    for f in sorted(rd.glob("results-*.jsonl")):
        for line in f.read_text().splitlines():
            try:
                recs.append(json.loads(line))
            except json.JSONDecodeError:
                try:
                    recs.append(json.loads(line.replace('"mbps":,', '"mbps":0,')))
                except json.JSONDecodeError:
                    pass
    return recs


def index(recs: list[dict]) -> dict:
    """按 proto 去掉 frp- 前缀归一化,便于两侧同协议对齐。"""
    idx: dict = {}
    for r in recs:
        proto = str(r.get("proto", "?")).replace("frp-", "")
        m = r["metric"]
        if m == "throughput":
            idx.setdefault(proto, {}).setdefault("th", {})[
                (r["path"], r["dir"], r["streams"])] = r["mbps"]
        elif m == "latency":
            idx.setdefault(proto, {}).setdefault("lat", {})[r["path"]] = r.get("data", {})
        elif m == "latency_newconn":
            idx.setdefault(proto, {}).setdefault("nc", r.get("data", {}))
        elif m in ("connect_rate", "udp", "http", "concurrent"):
            idx.setdefault(proto, {}).setdefault(m, []).append(r)
    return idx


def cell(v):
    return "—" if v in (None, 0, {}, []) and v != 0 else str(v)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("rh_dir")
    ap.add_argument("frp_dir")
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    rh = index(load_dir(Path(args.rh_dir)))
    fp = index(load_dir(Path(args.frp_dir)))
    protos = [p for p in ["tcp", "tls", "ws", "wss", "quic", "kcp"]
              if p in rh or p in fp]

    out: list[str] = []
    out.append("# rabbithole vs frp 跨机性能对比\n")
    out.append(f"- rabbithole v0.3.0(release)vs frp v0.71.0(official)")
    out.append(f"- 拓扑/工具/场景/端口完全一致;每指标带直连基线\n")

    # ---------- 吞吐 ----------
    out.append("## 吞吐(Mbps)\n")
    out.append("| 协议 | 方向×流 | 直连 | rabbithole | frp | RH/frp | RH保留率 | frp保留率 |")
    out.append("|---|---|---:|---:|---:|---:|---:|---:|")
    for proto in protos:
        rth = rh.get(proto, {}).get("th", {})
        fth = fp.get(proto, {}).get("th", {})
        keys = sorted(set(rth) | set(fth))
        for path, direction, streams in keys:
            if path != "tunnel":
                continue
            base = rth.get(("direct", direction, streams)) or fth.get(("direct", direction, streams)) or 0
            rv = rth.get(("tunnel", direction, streams))
            fv = fth.get(("tunnel", direction, streams))
            if rv is None and fv is None:
                continue
            ratio = f"{rv/fv:.2f}" if rv and fv else "—"
            rr = f"{rv/base*100:.0f}%" if rv and base else "—"
            fr = f"{fv/base*100:.0f}%" if fv and base else "—"
            out.append(f"| {proto} | {direction}×{streams} | {base} | "
                       f"{rv if rv is not None else '✗'} | {fv if fv is not None else '✗'} | "
                       f"{ratio} | {rr} | {fr} |")

    # ---------- 延迟 ----------
    out.append("\n## 往返延迟(ms, 5000 次)\n")
    out.append("| 协议 | 基线p50 | RH p50 | frp p50 | 基线p99 | RH p99 | frp p99 | RH新建p50 | frp新建p50 |")
    out.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for proto in protos:
        rlat = rh.get(proto, {}).get("lat", {})
        flat = fp.get(proto, {}).get("lat", {})
        b = rlat.get("direct") or flat.get("direct") or {}
        r, f = rlat.get("tunnel", {}), flat.get("tunnel", {})
        if not r and not f:
            continue
        rnc = rh.get(proto, {}).get("nc", {})
        fnc = fp.get(proto, {}).get("nc", {})
        out.append(
            f"| {proto} | {b.get('p50_ms','—')} | "
            f"{r.get('p50_ms','✗')} | {f.get('p50_ms','✗')} | "
            f"{b.get('p99_ms','—')} | {r.get('p99_ms','✗')} | {f.get('p99_ms','✗')} | "
            f"{rnc.get('p50_ms','—')} | {fnc.get('p50_ms','—')} |")

    # ---------- 建连 / UDP / HTTP ----------
    out.append("\n## 建连速率 / UDP / HTTP\n")
    out.append("| 协议 | 指标 | 直连 | rabbithole | frp | RH/frp |")
    out.append("|---|---|---:|---:|---:|---:|")
    for proto in protos:
        for metric, field in (("建连速率/s", "connect_rate"), ("UDP Mbps", "udp"), ("HTTP QPS", "http")):
            rr = rh.get(proto, {}).get(metric.split()[-1] if False else field, [])
            fr_ = fp.get(proto, {}).get(field, [])
            r = rr[-1] if rr else None
            f = fr_[-1] if fr_ else None
            if not r and not f:
                continue
            rd_ = (r or {}).get("direct", {})
            rt = (r or {}).get("tunnel", {})
            fd_ = (f or {}).get("direct", {})
            ft = (f or {}).get("tunnel", {})
            if field == "connect_rate":
                bv = rd_.get("rate_per_s", 0)
                rv, fv = rt.get("rate_per_s", 0), ft.get("rate_per_s", 0)
            elif field == "udp":
                bv = rd_.get("mbps", 0)
                rv, fv = rt.get("mbps", 0), ft.get("mbps", 0)
            else:
                bv = rd_.get("qps", 0)
                rv, fv = rt.get("qps", 0), ft.get("qps", 0)
            ratio = f"{rv/fv:.2f}" if rv and fv else "—"
            extra = ""
            if field == "udp":
                extra = f"(丢包 RH {rt.get('loss_pct','-')}% / frp {ft.get('loss_pct','-')}%)"
            out.append(f"| {proto} | {metric} | {bv} | {rv} | {fv} | {ratio} {extra} |")

    # ---------- 并发 ----------
    out.append("\n## 并发连接(仅 tcp 轮)\n")
    out.append("| 并发 | 直连成功 | RH 成功 | frp 成功 | RH 建连p99 | frp 建连p99 |")
    out.append("|---:|---:|---:|---:|---:|---:|")
    rcon = {r["n"]: r for r in rh.get("tcp", {}).get("concurrent", [])}
    fcon = {r["n"]: r for r in fp.get("tcp", {}).get("concurrent", [])}
    for n in sorted(set(rcon) | set(fcon)):
        r, f = rcon.get(n), fcon.get(n)
        rd = (r or f).get("direct", {})
        rt = (r or {}).get("tunnel", {})
        ft = (f or {}).get("tunnel", {})
        out.append(
            f"| {n} | {rd.get('established', '—')} "
            f"| {rt.get('established', '—')} | {ft.get('established', '—')} | "
            f"{rt.get('connect_p99_ms', '—')} | {ft.get('connect_p99_ms', '—')} |")

    md = "\n".join(out) + "\n"
    outdir = Path(args.out) if args.out else Path(args.frp_dir).parent / "compare"
    outdir.mkdir(parents=True, exist_ok=True)
    (outdir / "COMPARE.md").write_text(md)
    print(md)
    print(f"\n已写入 {outdir/'COMPARE.md'}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
