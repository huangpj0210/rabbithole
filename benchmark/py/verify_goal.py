#!/usr/bin/env python3
"""verify_goal —— goal-repair 性能验收证据核验器(fail closed,r2)。

用法:
  python3 py/verify_goal.py --evidence <dir>     # 核验证据目录(或其父目录)
  python3 py/verify_goal.py --self-test          # 对抗性自检:每个"旧口径/缺证据/
                                                 # 伪造证据"目录都必须判 FAIL

核验的是**完整证据链**,不是字段存在性,更不是 agents 手填的 pass 字段
(记录里的 pass/passed/verdict/gate_result 一律忽略并计数报告)。

证据目录 schema(goal-repair-evidence v1),由 peer.sh 三角色 +
py/build_manifest.py 产生、运行者回收组装:

  <dir>/
    schema.json                          {"schema":"goal-repair-evidence","version":1,...}
    results-<label>-<stamp>.jsonl        drive 记录(含 meta 与逐样本记录)
    raw/                                 每条 iperf3 的 .json/.stderr/.exit
    endpoints/server/{endpoint.json,identity.jsonl,cpu.jsonl,nofile.json}
    endpoints/client/{endpoint.json,identity.jsonl,cpu.jsonl,nofile.json}
    build/{sources.sha256,binaries.json[,bin/rh-server,bin/rh-client]}

r2 新增(响应主代理独立复现的 6 个误通过):
  * G4 硬检查 packet_size == 1200(不是"字段存在"而是"值正确");
  * G5 = 六协议(tcp/tls/ws/wss/quic/kcp,mux=1)各 4096 条 verified 隧道
    + tcp(mux=0)4096 对照;TLS 专项只是六分之一,不能替代其余协议;
  * build/sources.sha256 必须绑定真实源码树:按 binaries.json.repo_root
    (或 RH_VERIFY_REPO)逐条重算哈希,路径必须存在且哈希一致;跨目录
    manifest 必须完全一致(同一份构建,不许拼装);
  * concurrent 必须带完整保持证据(initial_verified/hold_alive/hold_rounds/
    alive_by_round),且字段间自洽、alive_by_round 每轮 ≥ verified;
  * raw iperf JSON 的 start.test_start 必须与记录一致:protocol=TCP、
    num_streams==streams、reverse==方向、duration==meta 时长;start.timestamp
    落在记录时间窗起点 ±5s;start.connected 的 remote_host/remote_port 必须
    等于记录的 dst/dst_port(记录声明隧道、实测跑直连这类错配直接 FAIL);
  * nofile 实值门控:端点 nofile.json 与 meta.drive_nofile_soft 都必须
    ≥ 并发档+512 且 ok==true("unlimited" 视为满足);
  * CPU 按样本自身所在目录匹配(不跨目录混配),G1 上行/下行分别 <50% 单核;
  * 拒绝 NaN/Infinity(JSON 非有限数按坏行处理)、同 key 重复 sample id/
    重复时间窗冒充多次独立测量、未排空 UDP(drain_seconds≥1)。

门槛(docs/performance-repair-acceptance.md §1,阈值不随测量方法改变):
  G1 tcp(mux=0,relay=splice)单流上传与下载各 ≥3000 Mbps,
     且同期 rh-server/rh-client 各 <50% 单核(按方向分别判定;覆盖率 ≥95%)
  G2 tls(mux=1)单流上传与下载各 ≥2500 Mbps
  G3 tcp/tls/ws/wss/quic/kcp 逐协议单流正吞吐(任一协议缺失即 FAIL)
  G4 逐协议 1200B UDP 损失 <1%(排空后去重口径;packet_size 必须 ==1200)
  G5 六协议各 4096 条 verified(应用层往返+保持存活)隧道 + tcp 裸连接对照
  G6 逐协议×方向×流数(1/4/16)与同轮配对直连基线保留率 ≥80%
  G7 每项数字 = ≥3 次独立配对测量的中位数(不足 = FAIL,不是 skip)
"""
from __future__ import annotations

import hashlib
import json
import math
import os
import re
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import source_inventory  # noqa: E402 (同目录共享模块:清单枚举单一事实来源)

SCHEMA = "goal-repair-evidence"
SCHEMA_VERSION = 1
PROTOS = ("tcp", "tls", "ws", "wss", "quic", "kcp")
DIRECTIONS = ("upload", "download")
STREAM_TIERS = (1, 4, 16)

# ★阈值固定:修测量方法不改变 threshold(docs §1 原表)
THRESH = {
    "g1_mbps": 3000.0,
    "g1_cpu_pct": 50.0,
    "g2_mbps": 2500.0,
    "g4_loss_pct": 1.0,
    "g4_packet_size": 1200,
    "g5_connections": 4096,
    "g6_retention": 0.80,
    "g7_min_pairs": 3,
    "cpu_coverage_min": 0.95,
    "nofile_margin": 512,
    "raw_ts_tolerance_s": 5.0,
    "raw_window_overhead_s": 15.0,
}

# 手填判定字段:核验器一律无视
HAND_FILLED_FIELDS = ("pass", "passed", "verdict", "gate_result", "result_ok")

HEX64 = re.compile(r"^[0-9a-f]{64}$")

# 源哈希重算缓存(跨目录共享同一文件路径)
_SHA_CACHE: dict[str, str] = {}


def median(vals):
    if not vals:
        return None
    s = sorted(vals)
    mid = len(s) // 2
    return s[mid] if len(s) % 2 else (s[mid - 1] + s[mid]) / 2


def is_num(v) -> bool:
    """有限数值(bool 不算):NaN/Infinity/None/str 全部 False。"""
    return isinstance(v, (int, float)) and not isinstance(v, bool) and math.isfinite(v)


def _reject_constant(tok):
    raise ValueError(f"非有限数字(不接受 NaN/Infinity): {tok}")


def strict_json_loads(text: str):
    """json.loads 但拒绝 NaN/Infinity/-Infinity(fail closed)。"""
    return json.loads(text, parse_constant=_reject_constant)


# ---------------------------------------------------------------------------
# 载入
# ---------------------------------------------------------------------------

def load_jsonl(path):
    recs, bad = [], []
    with open(path, encoding="utf-8") as fh:
        for no, line in enumerate(fh, 1):
            line = line.strip()
            if not line:
                continue
            try:
                recs.append(strict_json_loads(line))
            except (ValueError, json.JSONDecodeError):
                bad.append({"file": os.path.basename(path), "line": no})
    return recs, bad


def load_json(path):
    try:
        with open(path, encoding="utf-8") as fh:
            return strict_json_loads(fh.read())
    except (OSError, ValueError, json.JSONDecodeError):
        return None


def sha256_file(path):
    cached = _SHA_CACHE.get(path)
    if cached:
        return cached
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 20), b""):
            h.update(chunk)
    _SHA_CACHE[path] = h.hexdigest()
    return h.hexdigest()


def load_cpu(path):
    """cpu.jsonl → (header, samples);文件缺失/头缺失 → (None, [])。"""
    if not os.path.isfile(path):
        return None, []
    header, samples = None, []
    with open(path, encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                rec = strict_json_loads(line)
            except (ValueError, json.JSONDecodeError):
                continue
            if header is None and "clk_tck" in rec:
                header = rec
                continue
            if not is_num(rec.get("ticks")) or not is_num(rec.get("t_us")):
                continue
            samples.append(rec)
    samples.sort(key=lambda s: s["t_us"])
    return header, samples


def cpu_window(header, samples, w0_us, w1_us):
    """§1.1:窗口内样本数/期望 ≥95% 为覆盖;CPU% = Δticks/CLK_TCK/窗口秒×100。"""
    if not header or not samples or not (is_num(w0_us) and is_num(w1_us)) or w1_us <= w0_us:
        return {"coverage": 0.0, "cpu_pct": None, "reason": "无有效窗口/样本"}
    hz = header.get("hz")
    clk = header.get("clk_tck")
    if not is_num(hz) or hz <= 0 or not is_num(clk) or clk <= 0:
        return {"coverage": 0.0, "cpu_pct": None, "reason": "cpu 头缺 hz/clk_tck"}
    inside = [s for s in samples if w0_us <= s["t_us"] <= w1_us]
    expected = max(1.0, (w1_us - w0_us) / 1e6 * hz)
    coverage = min(1.0, len(inside) / expected)
    if len(inside) < 2:
        return {"coverage": coverage, "cpu_pct": None, "reason": "窗口内样本不足"}
    span_s = (inside[-1]["t_us"] - inside[0]["t_us"]) / 1e6
    if span_s <= 0:
        return {"coverage": coverage, "cpu_pct": None, "reason": "窗口时长为 0"}
    cpu_pct = (inside[-1]["ticks"] - inside[0]["ticks"]) / (clk * span_s) * 100.0
    if not math.isfinite(cpu_pct):
        return {"coverage": coverage, "cpu_pct": None, "reason": "CPU% 非有限值"}
    return {"coverage": coverage, "cpu_pct": round(cpu_pct, 2), "reason": ""}


# ---------------------------------------------------------------------------
# 单个证据目录核验(schema 层 + 完整证据链)
# ---------------------------------------------------------------------------

class EvidenceDir:
    """一个证据目录的完整解析结果。issues 非空 = 目录不合格 → 全部门槛 FAIL。"""

    def __init__(self, path):
        self.path = path
        self.name = os.path.basename(os.path.normpath(path))
        self.issues: list[str] = []
        self.hand_filled = 0
        self.metas: list[dict] = []
        self.throughput: list[dict] = []
        self.udp: list[dict] = []
        self.concurrent: list[dict] = []
        self.invalid_records: list[dict] = []
        self.endpoints: dict[str, dict] = {}
        self.cpu: dict[str, tuple] = {}       # role → (header, samples)
        self.config = None                    # (proto, mux, relay)
        self.run_id = None
        self.duration_s = None
        self.max_conc = None
        self.binaries_hashes: dict[str, str] = {}
        self.sources_manifest: str = ""
        self.build_binding: str = ""          # r3:已核验的源快照描述
        self._parse()

    # ---- 内部 ----
    def _issue(self, msg):
        self.issues.append(msg)

    ANCHOR_BEFORE_S = 300        # started_at 之前的容忍(时钟/序列化余量)
    ANCHOR_AFTER_S = 6 * 3600    # 单个证据目录的合理执行上界(远大于任何真实轮)

    def _check_time_anchor(self, started_at):
        """★r5:meta.started_at 与全部测量窗口的绝对时间锚定。"""
        import datetime as _dt
        if not started_at or not isinstance(started_at, str):
            self._issue("meta.started_at 缺失/非字符串(无法锚定绝对时间,克隆+"
                        "平移时间戳类伪造不可检 → fail closed)")
            return
        try:
            t0 = _dt.datetime.fromisoformat(started_at.replace("Z", "+00:00"))
        except ValueError:
            self._issue(f"meta.started_at 非法 ISO 时间:{started_at!r}")
            return
        if t0.tzinfo is None:
            t0 = t0.replace(tzinfo=_dt.timezone.utc)
        base_us = t0.timestamp() * 1e6
        lo = base_us - self.ANCHOR_BEFORE_S * 1e6
        hi = base_us + self.ANCHOR_AFTER_S * 1e6
        checked = 0
        for r in self.throughput:
            if r.get("valid") is not True:
                continue
            w0 = r.get("window_start_us")
            if not is_num(w0):
                continue
            checked += 1
            if not (lo <= w0 <= hi):
                self._issue(f"吞吐窗口绝对时间越界(started_at={started_at},"
                            f"window_start_us={w0:.0f} 偏离锚点 >"
                            f"{self.ANCHOR_BEFORE_S}s 前/{self.ANCHOR_AFTER_S//3600}h 后;"
                            "疑为克隆/伪造后平移时间戳)")
        for r in self.udp:
            d = r.get("data", r)
            if r.get("ok") is False or "error" in d or "env_error" in d:
                continue
            w0 = d.get("started_us")
            if not is_num(w0):
                continue
            checked += 1
            if not (lo <= w0 <= hi):
                self._issue(f"udp 窗口绝对时间越界(started_at={started_at},"
                            f"started_us={w0:.0f} 偏离锚点;疑为克隆/伪造后平移时间戳)")
        for r in self.concurrent:
            if r.get("path") != "tunnel":
                continue
            w0 = r.get("started_us")
            if not is_num(w0):
                continue
            checked += 1
            if not (lo <= w0 <= hi):
                self._issue(f"concurrent 窗口绝对时间越界(started_at={started_at},"
                            f"started_us={w0:.0f} 偏离锚点;疑为克隆/伪造后平移时间戳)")
        if checked == 0:
            self._issue("无可锚定的测量窗口(全部记录缺窗口字段)")

    def _parse(self):
        p = self.path
        schema = load_json(os.path.join(p, "schema.json"))
        if not schema or schema.get("schema") != SCHEMA:
            self._issue("缺 schema.json 或 schema 不符(不是 goal-repair 证据目录)")
            return
        if schema.get("version") != SCHEMA_VERSION:
            self._issue(f"schema 版本不符:期望 {SCHEMA_VERSION},得到 {schema.get('version')}")

        # ★r4:schema.json 的 run_id 待 meta 解析后交叉核对(目录混装防护:
        # schema 来自 A 轮、results 来自 B 轮时被抓)
        self._schema_run_id = schema.get("run_id")

        # ---- results 文件(严格 JSON:NaN/Infinity 按坏行)----
        import glob
        files = sorted(glob.glob(os.path.join(p, "results-*.jsonl")))
        if not files:
            self._issue("无 results-*.jsonl")
            return
        recs = []
        for f in files:
            r, bad = load_jsonl(f)
            recs.extend(r)
            for b in bad:
                self._issue(f"坏行(含 NaN/Infinity 等非法 JSON):{b['file']}:{b['line']}")

        for r in recs:
            if isinstance(r, dict):
                for k in HAND_FILLED_FIELDS:
                    if k in r:
                        self.hand_filled += 1
            metric = r.get("metric") if isinstance(r, dict) else None
            if metric == "meta":
                self.metas.append(r)
            elif metric == "throughput":
                self.throughput.append(r)
            elif metric == "udp":
                self.udp.append(r)
            elif metric == "concurrent":
                self.concurrent.append(r)

        # ---- meta 一致性(A9:显式必填,无默认值暗中生效)----
        if not self.metas:
            self._issue("无 meta 记录")
            return
        keys = {(m.get("proto"), m.get("relay"), m.get("mux"), m.get("run_id"))
                for m in self.metas}
        if len(keys) != 1:
            self._issue(f"meta 配置不一致(多组 proto/relay/mux/run_id):{sorted(map(str, keys))}")
            return
        m0 = self.metas[0]
        proto, relay, mux, run_id = m0.get("proto"), m0.get("relay"), m0.get("mux"), m0.get("run_id")
        for field, val in (("proto", proto), ("relay", relay), ("mux", mux), ("run_id", run_id)):
            if not val or not isinstance(val, str):
                self._issue(f"meta.{field} 缺失/为空(必须显式)")
                return
        if not m0.get("client_version"):
            self._issue("meta.client_version 为空(客户端版本未上报 → 数据不可信,本轮无效)")
        if not m0.get("drive_iperf3"):
            self._issue("meta.drive_iperf3 缺失(drive 侧工具版本未记录)")
        if m0.get("cc_source") != "endpoints" or m0.get("lowat_source") != "endpoints":
            self._issue("meta 未声明 cc/lowat 来源为端点实况(cc_source/lowat_source)")
        if not is_num(m0.get("peer_duration_s")) or m0["peer_duration_s"] <= 0:
            self._issue("meta.peer_duration_s 缺失/非正数(raw 时长核对需要它)")
        else:
            self.duration_s = m0["peer_duration_s"]
        conc = m0.get("conc_levels")
        if not isinstance(conc, list) or not conc or not all(is_num(x) and x > 0 for x in conc):
            self._issue("meta.conc_levels 缺失/非正数列表(nofile 门控需要它)")
        else:
            self.max_conc = max(int(x) for x in conc)
        self.config = (proto, mux == "true" or mux is True, relay)
        self.run_id = run_id
        # ★r4:schema.json run_id 与 meta.run_id 必须一致(peer.sh 两处同值写入)
        if self._schema_run_id != run_id:
            self._issue(f"schema.json run_id={self._schema_run_id!r} 与 meta run_id="
                        f"{run_id!r} 不一致(schema 与 results 来自不同轮次,目录混装)")
        # ★r5:绝对时间锚(peer.sh 在下发测量前写 meta.started_at,drive 同机
        # 时钟)。全部测量窗口必须落在 [started_at−300s, started_at+6h]:
        # 克隆目录后全局平移窗口/时间戳(+24h 等)而 meta.started_at 未随之
        # 改写时被抓;缺失 started_at = 无法锚定绝对时间,fail closed。
        self._check_time_anchor(m0.get("started_at"))

        # ---- nofile 实值门控(环境错误 = 目录不合格,不进入任何门槛)----
        need = (self.max_conc + THRESH["nofile_margin"]) if self.max_conc else None
        if need is not None:
            soft = m0.get("drive_nofile_soft")
            soft_n = int(soft) if isinstance(soft, str) and soft.strip().isdigit() else soft
            if isinstance(soft, str) and soft.strip().lower() == "unlimited":
                pass
            elif is_num(soft_n) and soft_n >= need:
                pass
            else:
                self._issue(f"环境错误:drive fd 软限={soft!r} < {need}(并发档"
                            f"{self.max_conc}+{THRESH['nofile_margin']};EMFILE 不得计入产品)")

        # ---- 每条记录 run_id 与 meta 一致 ----
        for r in self.throughput + self.udp + self.concurrent:
            if r.get("run_id") != run_id:
                self._issue(f"记录 run_id 与 meta 不一致:metric={r.get('metric')} "
                            f"run_id={r.get('run_id')}")
                break

        # ---- 端点实况证据 ----
        for role in ("server", "client"):
            ep_path = os.path.join(p, "endpoints", role, "endpoint.json")
            ep = load_json(ep_path)
            if not ep:
                self._issue(f"缺 endpoints/{role}/endpoint.json(端点实际配置证据)")
                continue
            self.endpoints[role] = ep
            if ep.get("run_id") != run_id:
                self._issue(f"endpoints/{role}.run_id={ep.get('run_id')!r} 与 meta "
                            f"run_id={run_id!r} 不一致")
            cfg = ep.get("config") or {}
            if str(cfg.get("proto")) != str(proto):
                self._issue(f"endpoints/{role}.config.proto={cfg.get('proto')!r} "
                            f"与 meta.proto={proto!r} 不一致(A9:元数据必须与端点实况一致)")
            if str(cfg.get("multiplex")).lower() != str(mux).lower():
                self._issue(f"endpoints/{role}.config.multiplex={cfg.get('multiplex')!r} "
                            f"与 meta.mux={mux!r} 不一致")
            if str(cfg.get("relay_mode")) != str(relay):
                self._issue(f"endpoints/{role}.config.relay_mode={cfg.get('relay_mode')!r} "
                            f"与 meta.relay={relay!r} 不一致")
            ident = ep.get("identity") or {}
            if not HEX64.match(str(ident.get("exe_sha256") or "")):
                self._issue(f"endpoints/{role}.identity.exe_sha256 缺失/非 SHA-256"
                            f"(运行的到底是哪个二进制无从证明)")
            env = ep.get("env") or {}
            for knob in ("RH_NOTSENT_LOWAT", "RH_QUIC_CC"):
                if knob not in env:
                    self._issue(f"endpoints/{role}.env 缺 {knob}(未设置也必须显式记 unset)")
            # nofile 实值(端点侧)
            nf = ep.get("nofile") or {}
            soft = str(nf.get("soft", ""))
            if soft.lower() == "unlimited":
                pass
            elif soft.isdigit() and int(soft) >= (need or 0):
                if nf.get("ok") is not True:
                    self._issue(f"endpoints/{role}.nofile.ok != true(soft={soft})")
            else:
                self._issue(f"环境错误:endpoints/{role} fd 软限={soft!r} < {need}"
                            f"(ok={nf.get('ok')};EMFILE 不得计入产品)")
        if "client" in self.endpoints and not self.endpoints["client"].get("iperf3"):
            self._issue("endpoints/client.iperf3 缺失(工具双端版本:B 侧 iperf3 未记录)")
        if "server" in self.endpoints and "client" in self.endpoints:
            s_env = self.endpoints["server"].get("env") or {}
            c_env = self.endpoints["client"].get("env") or {}
            for knob in ("RH_NOTSENT_LOWAT", "RH_QUIC_CC"):
                if knob in s_env and knob in c_env and s_env[knob] != c_env[knob]:
                    self._issue(f"端点 {knob} 两端不一致:server={s_env[knob]!r} "
                                f"client={c_env[knob]!r}(实验旋钮来源不一致)")

        # ---- CPU 采样证据 ----
        for role in ("server", "client"):
            header, samples = load_cpu(os.path.join(p, "endpoints", role, "cpu.jsonl"))
            self.cpu[role] = (header, samples)
            if header is None or not samples:
                self._issue(f"缺/空 endpoints/{role}/cpu.jsonl(G1 CPU 与 G5 RSS 的硬性依据)")

        # ---- build 清单(绑定真实源码树 + 运行二进制)----
        self._check_build(p)

        # ---- 吞吐原始留痕与形态核对(A11 + r2:方向/流数/目标/时间窗)----
        self._check_throughput_raw(p)

        # ---- UDP(A8:排空后口径 + r2:packet_size 必须 ==1200)----
        self._check_udp()

        # ---- 并发(A12 + r2:完整保持证据与自洽)----
        self._check_concurrent()

    # ---- build ----
    def _check_build(self, p):
        src = os.path.join(p, "build", "sources.sha256")
        if not os.path.isfile(src):
            self._issue("缺 build/sources.sha256(构建源哈希清单)")
            return
        try:
            manifest_text = open(src, encoding="utf-8").read()
        except OSError:
            self._issue("build/sources.sha256 不可读")
            return
        self.sources_manifest = manifest_text
        lines = [ln for ln in manifest_text.splitlines() if ln.strip()]
        if not lines:
            self._issue("build/sources.sha256 为空")
        entries = []
        seen_rel: set = set()
        for ln in lines:
            m = re.match(r"^([0-9a-f]{64})  (\S.+)$", ln)
            if not m:
                self._issue(f"build/sources.sha256 行格式不符:{ln[:80]}")
                return
            rel = m.group(2).strip()
            if rel in seen_rel:
                self._issue(f"清单重复条目:{rel}(同一文件两次,伪造覆盖面的常见手法)")
            seen_rel.add(rel)
            entries.append((m.group(1), rel))

        bj = load_json(os.path.join(p, "build", "binaries.json"))
        if not bj or not isinstance(bj.get("binaries"), dict):
            self._issue("缺/坏 build/binaries.json")
            return
        hashes = {}
        for b in ("rh-server", "rh-client"):
            h = ((bj.get("binaries") or {}).get(b) or {}).get("sha256", "")
            if not HEX64.match(str(h)):
                self._issue(f"build/binaries.json 缺 {b} 的 SHA-256")
            else:
                hashes[b] = h
            bin_copy = os.path.join(p, "build", "bin", b)
            if os.path.isfile(bin_copy):
                if sha256_file(bin_copy) != h:
                    self._issue(f"build/bin/{b} 哈希与 binaries.json 不符(拷贝损坏或被替换)")
        self.binaries_hashes = hashes
        # 运行二进制 === 构建二进制
        for role, b in (("server", "rh-server"), ("client", "rh-client")):
            ep = self.endpoints.get(role) or {}
            exe = str((ep.get("identity") or {}).get("exe_sha256") or "")
            if b in hashes and exe and exe != hashes[b]:
                self._issue(f"endpoints/{role} 实际运行二进制 SHA-256 与 build/binaries.json "
                            f"不一致(运行的不是这份构建)")

        # ★r3:源码树绑定 —— RH_VERIFY_REPO(实测验收固定为最终工作区)优先,
        # 证据自带的 repo_root 仅作回退:证据不能自带伪 repo 逃避最终工作区绑定。
        repo_env = os.environ.get("RH_VERIFY_REPO", "")
        repo = ""
        repo_source = ""
        if repo_env:
            if os.path.isdir(repo_env):
                repo, repo_source = repo_env, "RH_VERIFY_REPO"
            else:
                self._issue(f"RH_VERIFY_REPO={repo_env!r} 不是目录(验收工作区固定失败,"
                            f"不回退证据自带路径)")
        else:
            cand = str(bj.get("repo_root") or "")
            if os.path.isdir(cand):
                repo, repo_source = cand, "binaries.json.repo_root"
        if not repo:
            self._issue("无法定位源码树(未设 RH_VERIFY_REPO 且 binaries.json.repo_root "
                        "不可用)→ 源-构建绑定不可验证,fail closed;"
                        "实测验收请固定 export RH_VERIFY_REPO=<最终工作区>")
            return
        if not source_inventory.looks_like_cargo_repo(repo):
            self._issue(f"源树 {repo} 不像 Cargo 工作区(未找到任何 Cargo.toml,"
                        f"伪 repo 绑定拒绝)")
            return

        # ★r3:清单必须覆盖枚举规则给出的**全部**源文件(遗漏/多余都 FAIL)
        expected = set(source_inventory.enumerate_source_files(repo))
        listed = {rel for _h, rel in entries}
        missing = sorted(expected - listed)
        extra = sorted(listed - expected)
        if missing:
            show = ", ".join(missing[:5]) + ("…" if len(missing) > 5 else "")
            self._issue(f"清单遗漏 {len(missing)} 个源文件(枚举规则要求全部覆盖):{show}")
        if extra:
            self._issue(f"清单含 {len(extra)} 个枚举规则之外的条目:{', '.join(extra[:5])}")

        for want, rel in entries:
            if os.path.isabs(rel) or ".." in rel.split("/"):
                self._issue(f"sources.sha256 非法路径(逃逸):{rel}")
                continue
            fp = os.path.join(repo, rel)
            if os.path.islink(fp):
                self._issue(f"sources.sha256 条目是符号链接:{rel}(不可信构建输入)")
                continue
            if not os.path.isfile(fp):
                self._issue(f"sources.sha256 引用的文件不存在:{rel}(伪造/过期清单)")
                continue
            got = sha256_file(fp)
            if got != want:
                self._issue(f"源哈希不符:{rel}(清单记录 {want[:12]}…,当前 {got[:12]}…;"
                            f"运行构建与当前源码不一致)")
        # 已核验的源快照(供离线证据追溯:哪个 repo、多少文件、清单指纹)
        self.build_binding = (f"{repo}(来源:{repo_source};枚举 {len(expected)} 文件,"
                              f"清单 {len(listed)} 条,"
                              f"sha256={hashlib.sha256(manifest_text.encode()).hexdigest()[:16]}…)")

    # ---- 吞吐 ----
    def _check_throughput_raw(self, p):
        seen_ids: set = set()
        seen_windows: set = set()
        for r in self.throughput:
            tag = (f"label={r.get('label')} path={r.get('path')} dir={r.get('dir')} "
                   f"streams={r.get('streams')} sample={r.get('sample')}")
            valid = r.get("valid") is True
            if valid and not is_num(r.get("mbps")):
                self._issue(f"valid 吞吐样本 mbps 非有限数值:{tag}(mbps={r.get('mbps')!r})")
                continue
            if not valid:
                if not r.get("error"):
                    self._issue(f"无效吞吐样本缺 error 说明:{tag}")
                if r.get("mbps") is not None:
                    self._issue(f"无效吞吐样本 mbps 非 null(不得记 0 冒充):{tag}")
                self.invalid_records.append({"kind": "throughput_invalid", "rec": r})
            for field in ("raw_json", "raw_stderr", "raw_exit"):
                rel = r.get(field)
                if not rel or not os.path.isfile(os.path.join(p, rel)):
                    self._issue(f"吞吐样本缺 {field} 原始留痕:{tag} ({rel!r})")
            # 方向/流数来源一致:raw 文件名编码了 label/sample/path/dir/streams
            base = os.path.basename(r.get("raw_json") or "")
            want = f"thp-{r.get('label')}-s{r.get('sample')}-{r.get('path')}-" \
                   f"{r.get('dir')}-{r.get('streams')}s.json"
            if base != want:
                self._issue(f"raw 文件名与记录字段不符(A9 方向来源):{tag} → {base}")
            # 独立样本:同 key 不得重复 sample id / 重复时间窗
            for key_name, key_val in (("sample_id", r.get("sample")),
                                      ("window", (r.get("window_start_us"),
                                                  r.get("window_end_us")))):
                kk = (r.get("path"), r.get("dir"), r.get("streams"), key_name, key_val)
                if kk in seen_ids:
                    self._issue(f"重复 {key_name}(同 key 冒充多次独立测量):{tag}")
                seen_ids.add(kk)
            if not (is_num(r.get("streams")) and r.get("streams") in STREAM_TIERS):
                self._issue(f"streams 非法(必须 ∈ {STREAM_TIERS}):{tag}")
            # 后续核对需要 raw 文件真实存在;缺失时该记录问题已记录,继续下一条
            raw_path = os.path.join(p, str(r.get("raw_json") or ""))
            exit_path = os.path.join(p, str(r.get("raw_exit") or ""))
            if not (os.path.isfile(raw_path) and os.path.isfile(exit_path)):
                self.invalid_records.append({"kind": "throughput_invalid", "rec": r})
                continue
            try:
                exit_code = int(open(exit_path, encoding="utf-8").read().strip())
            except (OSError, ValueError):
                self._issue(f"raw_exit 不可读/非整数:{tag}")
                continue
            if exit_code != r.get("exit"):
                self._issue(f"raw_exit({exit_code}) 与记录 exit({r.get('exit')}) 不符:{tag}")
                continue
            raw = load_json(raw_path)
            if raw is None:
                self._issue(f"raw JSON 不可解析(或含 NaN/Infinity):{tag}")
                continue
            if valid:
                if exit_code != 0:
                    self._issue(f"valid=true 但 iperf 退出码 {exit_code}:{tag}")
                    continue
                try:
                    bps = raw["end"]["sum_received"]["bits_per_second"]
                except (TypeError, KeyError):
                    self._issue(f"raw JSON 缺 end.sum_received.bits_per_second:{tag}")
                    continue
                if not is_num(bps) or bps <= 0:
                    self._issue(f"raw JSON bits_per_second 非正有限数:{tag}")
                    continue
                want_mbps = int(bps / 1e6)  # peer.sh 用 floor
                if abs(r.get("mbps", -1) - want_mbps) > 1:
                    self._issue(f"记录 mbps({r.get('mbps')}) 与 raw 重算({want_mbps}) 不符:{tag}")
            else:
                if exit_code == 0 and not (r.get("error") or {}).get("reason"):
                    self._issue(f"exit=0 但标 invalid 且无原因:{tag}")
            # ★r2:raw 测试形态核对(有效样本才需要形态一致;无效样本留痕即可)
            if valid:
                self._check_raw_shape(raw, r, tag)

    def _check_raw_shape(self, raw, r, tag):
        """start.test_start / start.connected / start.timestamp 必须与记录一致。

        ★r3:timestamp 是真实 iperf3 的**标准对象形态**
        {"time": "...", "timesecs": int, "timemillisecs": int?}(3.22 实测如此,
        3.17.1 同族 schema,验收机现场再核对一次)。解析规则:
          * 有 timemillisecs(毫秒精度)优先使用;
          * 只有 timesecs(秒精度)时容差放宽 1s 吸收秒级截断;
          * 数值/字符串等非对象形态 = 伪造或手改,FAIL —— 不把对象强转伪数值。
        """
        start = raw.get("start") or {}
        ts_block = start.get("test_start")
        if not isinstance(ts_block, dict):
            self._issue(f"raw JSON 缺 start.test_start:{tag}")
            return
        proto = str(ts_block.get("protocol", "")).upper()
        if proto != "TCP":
            self._issue(f"raw test_start.protocol={proto!r} ≠ TCP(测试形态错配):{tag}")
        streams = ts_block.get("num_streams")
        if not is_num(streams) or int(streams) != int(r.get("streams", -1)):
            self._issue(f"raw num_streams={streams!r} 与记录 streams={r.get('streams')!r} "
                        f"不符(并发数来源不一致):{tag}")
        reverse = ts_block.get("reverse")
        want_rev = 1 if r.get("dir") == "download" else 0
        if not is_num(reverse) or int(reverse) != want_rev:
            self._issue(f"raw reverse={reverse!r} 与方向 {r.get('dir')!r}(期望 "
                        f"reverse={want_rev})不符:{tag}")
        dur = ts_block.get("duration")
        if self.duration_s is not None and (not is_num(dur)
                                            or abs(float(dur) - self.duration_s) > 0.5):
            self._issue(f"raw duration={dur!r} 与 meta.peer_duration_s="
                        f"{self.duration_s} 不符:{tag}")
        # 时间窗:iperf 自身时间戳必须落在窗口起点 ±容差;窗口长度 ≈ duration
        w0, w1 = r.get("window_start_us"), r.get("window_end_us")
        ts_obj = start.get("timestamp")
        ts_s = None
        tolerance = THRESH["raw_ts_tolerance_s"]
        if isinstance(ts_obj, dict):
            ms = ts_obj.get("timemillisecs")
            secs = ts_obj.get("timesecs")
            if is_num(ms) and ms > 0:
                ts_s = ms / 1000.0                    # 毫秒精度优先
            elif is_num(secs) and secs > 0:
                ts_s = float(secs)
                tolerance += 1.0                       # 秒精度:放宽 1s 截断误差
            else:
                self._issue(f"raw start.timestamp 对象缺可用的 timesecs/"
                            f"timemillisecs:{tag}")
        else:
            self._issue(f"raw start.timestamp 非标准对象形态(真实 iperf3 输出 "
                        f"{{time,timesecs[,timemillisecs]}},得到 {type(ts_obj).__name__}):{tag}")
        if ts_s is not None and is_num(w0):
            if abs(ts_s - w0 / 1e6) > tolerance:
                self._issue(f"raw start.timestamp({ts_s:.3f}s) 偏离窗口起点 "
                            f"{w0/1e6:.3f}s 超过 {tolerance}s:{tag}")
        if not is_num(w0) or not is_num(w1):
            self._issue(f"记录窗口时间戳缺失/非数值:{tag}")
        if is_num(w0) and is_num(w1):
            span = (w1 - w0) / 1e6
            if span <= 0 or span > self.duration_s + THRESH["raw_window_overhead_s"]:
                self._issue(f"窗口时长 {span:.1f}s 异常(≤0 或远超 duration="
                            f"{self.duration_s}s):{tag}")
        # 目标:connected 远端必须等于记录 dst/dst_port(声明隧道、实测直连 → FAIL)
        conn = start.get("connected")
        if not isinstance(conn, list) or not conn:
            self._issue(f"raw start.connected 缺失(无法核对测试目标):{tag}")
            return
        remote = conn[0] or {}
        dst, dst_port = r.get("dst"), r.get("dst_port")
        if not dst or not is_num(dst_port):
            self._issue(f"记录缺 dst/dst_port(无法与 raw connected 核对目标):{tag}")
            return
        if str(remote.get("remote_port")) != str(int(dst_port)):
            self._issue(f"raw connected.remote_port={remote.get('remote_port')!r} ≠ 记录 "
                        f"dst_port={dst_port}(测试目标错配):{tag}")
        if str(remote.get("remote_host")) != str(dst):
            self._issue(f"raw connected.remote_host={remote.get('remote_host')!r} ≠ 记录 "
                        f"dst={dst!r}(测试目标错配):{tag}")

    # ---- UDP ----
    def _check_udp(self):
        seen_samples: set = set()
        windows: dict[object, list[tuple]] = {}
        for r in self.udp:
            d = r.get("data", r)
            tag = f"label={r.get('label')} sample={r.get('sample')} path={r.get('path')}"
            if r.get("ok") is False or "error" in d or "env_error" in d:
                self.invalid_records.append({"kind": "udp_invalid", "rec": r})
                continue
            for field in ("sent", "recv_unique", "late_recv", "dup", "drain_seconds",
                          "loss_pct", "started_us", "ended_us"):
                if field not in d:
                    self._issue(f"udp 样本缺 {field}(旧口径:无序号去重/排空/时间窗"
                                f" → 损失口径与独立性均无效){tag}")
                    break
            ps = d.get("packet_size")
            if not is_num(ps) or int(ps) != THRESH["g4_packet_size"]:
                self._issue(f"udp packet_size={ps!r} ≠ {THRESH['g4_packet_size']}"
                            f"(G4 是 1200B 口径,不是'有字段就行'):{tag}")
                continue
            drain = d.get("drain_seconds")
            if not is_num(drain) or drain < 1:
                self._issue(f"udp drain_seconds={drain!r} <1(未排空口径,损失不可信):{tag}")
                continue
            sent, uniq = d.get("sent"), d.get("recv_unique")
            if not is_num(sent) or not is_num(uniq) or sent <= 0:
                self._issue(f"udp sent/recv_unique 非正有限数值:{tag}")
                continue
            lost = sent - uniq
            if lost < 0 or uniq > sent + (d.get("dup") or 0):
                self._issue(f"udp 样本数字不自洽:{tag} sent={sent} recv_unique={uniq} "
                            f"dup={d.get('dup')}")
                continue
            rec_loss = d.get("loss_pct")
            if not is_num(rec_loss):
                self._issue(f"udp loss_pct 非有限数值:{tag}")
                continue
            if abs(rec_loss - lost * 100.0 / sent) > 0.01:
                self._issue(f"udp loss_pct 与 (sent-recv_unique)/sent 不符:{tag}")
                continue
            # 独立样本:同 (path,sample) 不得重复;同 path 时间窗不得重叠
            k = (r.get("path"), r.get("sample"))
            if k in seen_samples:
                self._issue(f"udp 重复 sample(同 key 冒充多次独立测量):{tag}")
            seen_samples.add(k)
            w0, w1 = d.get("started_us"), d.get("ended_us")
            if is_num(w0) and is_num(w1) and w1 > w0:
                for (o0, o1) in windows.get(r.get("path"), []):
                    if w0 < o1 and o0 < w1:
                        self._issue(f"udp 样本时间窗重叠(独立测量的物理窗口不可能"
                                    f"重叠,复制记录冒充样本):{tag}")
                        break
                windows.setdefault(r.get("path"), []).append((w0, w1))
            else:
                self._issue(f"udp started_us/ended_us 非法(无法做独立性查重):{tag}")

    # ---- 并发 ----
    def _check_concurrent(self):
        seen_conc: set = set()
        conc_windows: dict[object, list[tuple]] = {}
        for r in self.concurrent:
            n = r.get("n")
            tag = f"label={r.get('label')} n={n} sample={r.get('sample')}"
            required = ("established", "initial_verified", "hold_alive", "verified",
                        "failed", "hold_rounds", "alive_by_round", "started_us", "ended_us")
            for field in required:
                if field not in r:
                    self._issue(f"concurrent 记录缺 {field}(旧口径/伪造保持:"
                                f"仅握手或只留结论字段不能证明持续存活){tag}")
                    break
            for field in ("established", "initial_verified", "hold_alive", "verified",
                          "failed", "hold_rounds", "started_us", "ended_us"):
                if field in r and not is_num(r.get(field)):
                    self._issue(f"concurrent.{field} 非有限数值:{tag}")
                    break
            abr = r.get("alive_by_round")
            if not isinstance(abr, list) or not abr:
                self._issue(f"concurrent.alive_by_round 缺失/为空(保持期持续存活证据):{tag}")
                continue
            if not all(is_num(x) for x in abr):
                self._issue(f"concurrent.alive_by_round 含非有限数值:{tag}")
                continue
            if len(abr) != r.get("hold_rounds"):
                self._issue(f"alive_by_round 长度 {len(abr)} ≠ hold_rounds="
                            f"{r.get('hold_rounds')}:{tag}")
                continue
            if r.get("hold_rounds", 0) < 1:
                self._issue(f"hold_rounds={r.get('hold_rounds')} <1(没有保持期):{tag}")
                continue
            if not (r.get("started_us", 0) < r.get("ended_us", 0)):
                self._issue(f"并发窗口时间戳非法:{tag}")
                continue
            # 字段自洽:verified ≤ hold_alive ≤ initial_verified ≤ established ≤ n
            est, iv, ha, ver = (r.get("established"), r.get("initial_verified"),
                                r.get("hold_alive"), r.get("verified"))
            if not (ver <= ha <= iv <= est <= (n if is_num(n) else est)):
                self._issue(f"concurrent 字段不自洽(需 verified≤hold_alive≤"
                            f"initial_verified≤established≤n):{tag} "
                            f"verified={ver} hold_alive={ha} initial_verified={iv} "
                            f"established={est} n={n}")
                continue
            if min(abr) < ver:
                self._issue(f"alive_by_round 每轮存活({min(abr)}) < verified({ver})"
                            f"(保持结果与逐轮证据矛盾,疑似伪造):{tag}")
                continue
            if max(abr) > est:
                self._issue(f"alive_by_round 最大值({max(abr)}) > established({est}):{tag}")
                continue
            # 独立样本:同 (path,n,sample) 不得重复;同 n 时间窗不得重叠。
            # ★修复(2026-10-13,round-1 实测):键必须含 path —— peer.sh 对同一 n
            # 成对执行 direct/tunnel(同 sample 号是 G7 配对设计,与 UDP/吞吐
            # 查重键口径一致),漏掉 path 会把合法配对误判为重复测量。
            # 防复制冒充语义不变:同 path 内同号样本、同 n 重叠窗口仍判 FAIL。
            k = (r.get("path"), r.get("n"), r.get("sample"))
            if k in seen_conc:
                self._issue(f"concurrent 重复 sample(同 key 冒充多次独立测量):{tag}")
            seen_conc.add(k)
            w0, w1 = r.get("started_us"), r.get("ended_us")
            for (o0, o1) in conc_windows.get(r.get("n"), []):
                if w0 < o1 and o0 < w1:
                    self._issue(f"concurrent 样本时间窗重叠(独立测量的物理窗口不可能"
                                f"重叠,复制记录冒充样本):{tag}")
                    break
            conc_windows.setdefault(r.get("n"), []).append((w0, w1))


# ---------------------------------------------------------------------------
# 门槛判定(跨目录聚合)
# ---------------------------------------------------------------------------

def _windows_overlap(a, b):
    """两个数值时间窗是否重叠(None/非法窗不判重叠,由各自字段检查负责)。"""
    a0, a1 = a
    b0, b1 = b
    if not all(is_num(x) for x in (a0, a1, b0, b1)):
        return False
    return a0 < b1 and b0 < a1


def cross_dir_independence(evid):
    """★r4/r5 跨目录独立性(克隆目录/复用同一物理测量的防线):

    固定三机拓扑串行执行 → 同一 (proto,mux,relay[,dir,streams,n],path) 的
    **独立测量物理时间窗不可能重叠**。整目录克隆或复制记录冒充多样本时,
    时间窗完全相同 → 重叠 → 涉事目录记 issue(目录不合格,全部门槛 FAIL)。
    覆盖吞吐/UDP/并发三类记录,且同时在目录内与目录间生效。

    ★r5 追加两道绝对锚(挡「克隆+全局时间平移」):
    * run_id 全局互异:README:201-203 规定每个证据目录用**新的 PEER_RUN_ID**
      (换协议/MUX/RELAY 须重启两端另开目录)——两个合格目录同 run_id
      = 同一轮被复制成两份;
    * 跨配置窗口互斥:同一三机拓扑上任何测量(不论协议/配置)都串行执行,
      全部合格目录的全部测量窗口必须两两不重叠(换协议需串行重启)。
    """
    # ★r4:同 key 时间窗重叠(目录内 + 目录间)——先于 run_id 检查执行,
    # 使克隆场景同时留下「窗口重叠」与「run_id 重复」两类证据
    groups: dict[tuple, list] = {}
    # ★r5-2:跨配置互斥(全部测量窗口两两不重叠)
    all_windows: list[tuple] = []
    for d in evid:
        if d is None or d.issues or d.config is None:
            continue
        proto, mux, relay = d.config
        for r in d.throughput:
            if r.get("valid") is not True:
                continue
            win = (r.get("window_start_us"), r.get("window_end_us"))
            groups.setdefault(("throughput", proto, mux, relay, r.get("path"),
                               r.get("dir"), r.get("streams")), []).append(
                (d, r.get("sample"), win))
            all_windows.append((d, r.get("sample"), win))
        for r in d.udp:
            data = r.get("data", r)
            if r.get("ok") is False or "error" in data or "env_error" in data:
                continue
            win = (data.get("started_us"), data.get("ended_us"))
            groups.setdefault(("udp", proto, mux, relay, r.get("path")), []).append(
                (d, r.get("sample"), win))
            all_windows.append((d, r.get("sample"), win))
        for r in d.concurrent:
            if r.get("path") != "tunnel":
                continue
            win = (r.get("started_us"), r.get("ended_us"))
            groups.setdefault(("concurrent", proto, mux, relay, r.get("n")), []).append(
                (d, r.get("sample"), win))
            all_windows.append((d, r.get("sample"), win))
    for k, items in groups.items():
        for i in range(len(items)):
            for j in range(i + 1, len(items)):
                di, si, wi = items[i]
                dj, sj, wj = items[j]
                if _windows_overlap(wi, wj):
                    what = k[0]
                    for d in {id(di): di, id(dj): dj}.values():
                        d._issue(f"跨目录/目录内独立性被破坏:{what} 样本时间窗重叠"
                                 f"(key={k[1:]};{di.name}#{si} 与 {dj.name}#{sj} —— "
                                 f"串行拓扑下独立测量窗口不可能重叠,疑为目录克隆或"
                                 f"记录复制)")
    # ★r5-2 判定:跨配置互斥(同 key 重叠已由上面报过,这里只报跨配置对)
    for i in range(len(all_windows)):
        for j in range(i + 1, len(all_windows)):
            di, si, wi = all_windows[i]
            dj, sj, wj = all_windows[j]
            if di is dj:
                continue  # 同目录内的重叠由同 key 检查与目录内查重负责
            if di.config == dj.config:
                continue  # 同配置由 r4 同 key 检查负责
            if _windows_overlap(wi, wj):
                for d in (di, dj):
                    d._issue(f"跨配置互斥被破坏:{di.name}#{si} 与 {dj.name}#{sj} "
                             f"测量窗口重叠(换协议/MUX/RELAY 须重启两端串行执行,"
                             "不同配置目录窗口不可能重叠)")

    # ★r5-1:run_id 全局互异(最后执行;窗口类证据已先行落档)
    seen_rid: dict[str, object] = {}
    for d in evid:
        if d is None or d.config is None or not d.run_id:
            continue
        if d.issues:
            continue
        if d.run_id in seen_rid:
            for dd in (seen_rid[d.run_id], d):
                dd._issue(f"run_id 重复:{d.run_id!r} 同时出现在 "
                          f"{seen_rid[d.run_id].name} 与 {d.name}(规程要求每个"
                          "证据目录使用新的 PEER_RUN_ID;同 run_id 双目录 = "
                          "同一轮被克隆)")
        else:
            seen_rid[d.run_id] = d


def collect_pairs(dirs):
    """聚合 (direct, tunnel) 配对样本;每样本的 CPU 取自**其所在目录**(不跨目录混配)。"""
    pairs: dict[tuple, list[dict]] = {}
    for d in dirs:
        if d.issues or d.config is None:
            continue
        proto, mux, relay = d.config
        by_key: dict[tuple, dict] = {}
        for r in d.throughput:
            if r.get("valid") is not True or not is_num(r.get("mbps")):
                continue
            k = (r.get("dir"), r.get("streams"), r.get("sample"))
            by_key.setdefault(k, {})[r.get("path")] = r
        for (direction, streams, sample), pv in by_key.items():
            if "direct" in pv and "tunnel" in pv:
                pk = (proto, mux, relay, direction, streams)
                tun = pv["tunnel"]
                item = {
                    "direct": pv["direct"]["mbps"], "tunnel": tun["mbps"],
                    "dir_path": d.name, "sample": sample,
                    "window": (tun.get("window_start_us"), tun.get("window_end_us")),
                    # ★CPU 按样本自身目录计算(r2:拒绝同配置多目录交叉混配)
                    "cpu": {role: cpu_window(*d.cpu.get(role, (None, [])),
                                             tun.get("window_start_us"),
                                             tun.get("window_end_us"))
                            for role in ("server", "client")},
                }
                pairs.setdefault(pk, []).append(item)
    return pairs


def collect_udp(dirs):
    out: dict[tuple, list[dict]] = {}
    for d in dirs:
        if d.issues or d.config is None:
            continue
        proto, mux, relay = d.config
        for r in d.udp:
            data = r.get("data", r)
            if r.get("ok") is False or "error" in data or "env_error" in data:
                continue
            if r.get("path") != "tunnel":
                continue
            out.setdefault((proto, mux, relay), []).append(data)
    return out


def collect_concurrent(dirs):
    out: dict[tuple, list[dict]] = {}
    for d in dirs:
        if d.issues or d.config is None:
            continue
        proto, mux, relay = d.config
        for r in d.concurrent:
            if r.get("path") != "tunnel":
                continue
            out.setdefault((proto, mux, relay), []).append((r, d))
    return out


class Gate:
    def __init__(self, gid, title):
        self.gid, self.title = gid, title
        self.ok = True
        self.details: list[str] = []

    def fail(self, msg):
        self.ok = False
        self.details.append(msg)

    def note(self, msg):
        self.details.append(msg)


def evaluate_gates(dirs):
    pairs = collect_pairs(dirs)
    udps = collect_udp(dirs)
    concs = collect_concurrent(dirs)
    min_pairs = THRESH["g7_min_pairs"]

    def pair_items(proto, mux, relay, direction, streams):
        return pairs.get((proto, mux, relay, direction, streams), [])

    def med_tunnel(proto, mux, relay, direction, streams):
        items = pair_items(proto, mux, relay, direction, streams)
        return median([i["tunnel"] for i in items]), items

    gates = []

    # ---- G1(CPU 按方向分别判定,样本自带目录的 CPU,不混配)----
    g1 = Gate("G1", f"TCP nomux(mux=0,relay=splice)单流双向各 ≥{THRESH['g1_mbps']:.0f} Mbps"
                    f" + 双端 CPU 上/下行分别 <{THRESH['g1_cpu_pct']:.0f}% 单核")
    g1_cfg = ("tcp", False, "splice")
    if not [d for d in dirs if d.config == g1_cfg and not d.issues]:
        g1.fail("没有 (tcp, mux=false, relay=splice) 证据目录")
    for direction in DIRECTIONS:
        med, items = med_tunnel(*g1_cfg, direction, streams=1)
        if not items:
            g1.fail(f"{direction}:无有效配对样本")
            continue
        if len(items) < min_pairs:
            g1.fail(f"{direction}:配对样本 {len(items)} < {min_pairs}(G7)")
        if med is None or med < THRESH["g1_mbps"]:
            g1.fail(f"{direction}:单流中位数 {med} < {THRESH['g1_mbps']:.0f} Mbps")
        else:
            g1.note(f"{direction}:单流中位数 {med:.0f} Mbps(n={len(items)})")
        # ★该方向自己的 CPU 样本(上/下行分别判定)
        for role in ("server", "client"):
            cpus = []
            for i in items:
                c = i["cpu"][role]
                if c["coverage"] < THRESH["cpu_coverage_min"]:
                    g1.fail(f"{direction} sample={i['sample']}({i['dir_path']}) {role} "
                            f"CPU 覆盖率 {c['coverage']*100:.1f}% < 95%(样本判 invalid)")
                elif c["cpu_pct"] is None:
                    # ★r4 fail-closed:覆盖达标但 CPU 不可计算(窗口内样本<2/
                    # span=0/非有限)只在证据被操纵的参数下可达——不得静默跳过
                    g1.fail(f"{direction} sample={i['sample']}({i['dir_path']}) {role} "
                            f"CPU 覆盖率达标但 CPU% 不可计算({c.get('reason')})"
                            f"(样本判 invalid)")
                else:
                    cpus.append(c["cpu_pct"])
            m = median(cpus)
            if not cpus:
                g1.fail(f"{direction} {role}:无可用 CPU 样本(cpu.jsonl 缺失或覆盖率全不足)")
            elif m >= THRESH["g1_cpu_pct"]:
                g1.fail(f"{direction} {role}:CPU 中位数 {m}% ≥ {THRESH['g1_cpu_pct']:.0f}% 单核")
            else:
                g1.note(f"{direction} {role}:CPU 中位数 {m}%")
    gates.append(g1)

    # ---- G2 ----
    g2 = Gate("G2", f"TLS mux 单流双向各 ≥{THRESH['g2_mbps']:.0f} Mbps")
    g2_cfg = ("tls", True, "splice")
    if not [d for d in dirs if d.config == g2_cfg and not d.issues]:
        g2.fail("没有 (tls, mux=true, relay=splice) 证据目录")
    for direction in DIRECTIONS:
        med, items = med_tunnel(*g2_cfg, direction, streams=1)
        if not items:
            g2.fail(f"{direction}:无有效配对样本")
            continue
        if len(items) < min_pairs:
            g2.fail(f"{direction}:配对样本 {len(items)} < {min_pairs}(G7)")
        if med is None or med < THRESH["g2_mbps"]:
            g2.fail(f"{direction}:单流中位数 {med} < {THRESH['g2_mbps']:.0f} Mbps")
        else:
            g2.note(f"{direction}:单流中位数 {med:.0f} Mbps(n={len(items)})")
    gates.append(g2)

    # ---- G3(六协议逐个,不允许互相顶替;只认最终版本 relay=splice)----
    g3 = Gate("G3", "tcp/tls/ws/wss/quic/kcp 逐协议单流正吞吐(mux=true,relay=splice)")
    for proto in PROTOS:
        any_items = False
        for direction in DIRECTIONS:
            med, items = med_tunnel(proto, True, "splice", direction, streams=1)
            if not items:
                continue
            any_items = True
            if len(items) < min_pairs:
                g3.fail(f"{proto}/{direction}:配对样本 {len(items)} < {min_pairs}(G7)")
            if med is None or med <= 0:
                g3.fail(f"{proto}/{direction}:单流中位数 {med}(非正吞吐)")
        if not any_items:
            g3.fail(f"{proto}:无 (mux=true,relay=splice) 证据"
                    f"(TLS 专项或任一协议的结果不能替代)")
    gates.append(g3)

    # ---- G4(排空后口径 + packet_size==1200 已在目录层硬检查)----
    g4 = Gate("G4", f"逐协议 1200B UDP 损失 <{THRESH['g4_loss_pct']}%(排空后去重)")
    for proto in PROTOS:
        samples = udps.get((proto, True, "splice"), [])
        if len(samples) < min_pairs:
            g4.fail(f"{proto}:有效隧道 UDP 样本 {len(samples)} < {min_pairs}")
            continue
        loss = median([s.get("loss_pct", 100.0) for s in samples])
        if loss is None or loss >= THRESH["g4_loss_pct"]:
            g4.fail(f"{proto}:损失中位数 {loss}% ≥ {THRESH['g4_loss_pct']}%")
        else:
            g4.note(f"{proto}:损失中位数 {loss}%(n={len(samples)})")
    gates.append(g4)

    # ---- G5(★r2:六协议各 4096 verified + tcp 裸连接对照)----
    g5 = Gate("G5", "六协议(tcp/tls/ws/wss/quic/kcp,mux=1)各 "
                    f"{THRESH['g5_connections']} 条 verified 隧道 + tcp(mux=0)对照"
                    f"(verified=应用层往返+逐轮保持存活)")
    n_need = THRESH["g5_connections"]

    def check_conc(cfg, label):
        recs = []
        for key, rs in concs.items():
            if key[:3] == cfg:
                recs.extend([(r, d) for r, d in rs if r.get("n") == n_need])
        if not recs:
            g5.fail(f"{label}:无 n={n_need} 的 tunnel concurrent 记录")
            return
        if len(recs) < min_pairs:
            g5.fail(f"{label}:4096 并发样本 {len(recs)} < {min_pairs}(G7)")
        ver = [r.get("verified", 0) for r, _d in recs]
        est = [r.get("established", 0) for r, _d in recs]
        if median(est) != n_need:
            g5.fail(f"{label}:established 中位数 {median(est)} ≠ {n_need}")
        if min(ver) != n_need or median(ver) != n_need:
            g5.fail(f"{label}:verified={sorted(ver)}(存在未达 {n_need} 的样本;"
                    f"仅握手 established 不能算可用)")
        else:
            g5.note(f"{label}:verified={n_need}×{len(recs)} "
                    f"(alive_by_round 已逐轮核对)")
        # RSS 记录(§1.2:验收记录两端 RSS;取样本所在目录,不混配)
        for r, d in recs:
            w0, w1 = r.get("started_us"), r.get("ended_us")
            if not (is_num(w0) and is_num(w1)):
                g5.note(f"{label}:并发记录缺窗口时间戳,RSS 无法对齐")
                continue
            for role in ("server", "client"):
                header, samples = d.cpu.get(role, (None, []))
                inside = [s for s in samples if w0 <= s["t_us"] <= w1]
                if inside:
                    g5.note(f"{label}/{role} 窗口内 RSS 峰值 "
                            f"{max(s.get('rss_kb', 0) for s in inside)/1024:.0f} MiB")

    for proto in PROTOS:
        check_conc((proto, True, "splice"), f"{proto}-mux")
    check_conc(("tcp", False, "splice"), "tcp-nomux 对照")
    gates.append(g5)

    # ---- G6(逐协议×方向×流数配对保留率;单流不得以多流顶替)----
    g6 = Gate("G6", f"逐协议×方向×流数(1/4/16)配对保留率 ≥{THRESH['g6_retention']*100:.0f}%")
    for proto in PROTOS:
        for direction in DIRECTIONS:
            for streams in STREAM_TIERS:
                ratios = [i["tunnel"] / i["direct"]
                          for i in pair_items(proto, True, "splice", direction, streams)
                          if is_num(i["direct"]) and i["direct"] > 0]
                if len(ratios) < min_pairs:
                    g6.fail(f"{proto}/{direction}/{streams}流:配对 {len(ratios)} < {min_pairs}"
                            f"(G7;直连基线无效也算配对缺失)")
                    continue
                m = median(ratios)
                if m is None or not math.isfinite(m) or m < THRESH["g6_retention"]:
                    g6.fail(f"{proto}/{direction}/{streams}流:保留率中位数 "
                            f"{m*100 if m is not None else m}% < {THRESH['g6_retention']*100:.0f}%")
    gates.append(g6)

    # ---- G7 ----
    invalid_total = sum(len(d.invalid_records) for d in dirs)
    g7 = Gate("G7", f"≥{min_pairs} 次独立配对测量的中位数;全部样本与失败逐条保存")
    for d in dirs:
        if d.issues:
            continue
        raw_dir = os.path.join(d.path, "raw")
        if not os.path.isdir(raw_dir) or not os.listdir(raw_dir):
            g7.fail(f"{d.name}:raw/ 为空(原始样本未保存)")
    g7.note(f"无效/失败样本共 {invalid_total} 条(已在各目录留痕并计入配对缺口)")
    gates.append(g7)

    return gates


# ---------------------------------------------------------------------------
# 顶层入口
# ---------------------------------------------------------------------------

def discover_evidence_dirs(path):
    """path 本身是证据目录,或其子目录们是(跳过非证据目录并注明)。"""
    found, skipped = [], []
    if os.path.isfile(os.path.join(path, "schema.json")):
        return [path], []
    for name in sorted(os.listdir(path)):
        sub = os.path.join(path, name)
        if not os.path.isdir(sub):
            continue
        if os.path.isfile(os.path.join(sub, "schema.json")):
            found.append(sub)
        else:
            skipped.append(name)
    return found, skipped


def verify_path(path):
    """返回 (ok, report)。ok=True 仅当:无目录级问题、跨目录构建一致、G1-G7 全过。"""
    lines = []
    if not os.path.isdir(path):
        return False, f"不是目录: {path}"
    dirs, skipped = discover_evidence_dirs(path)
    lines.append(f"== goal-repair 验收核验({path})==")
    lines.append(f"证据目录 {len(dirs)} 个" + (f";跳过(无 schema.json):{skipped}" if skipped else ""))
    if not dirs:
        lines.append("FAIL:未发现任何 goal-repair 证据目录(缺 schema.json)")
        return False, "\n".join(lines)

    evid = []
    for p in dirs:
        try:
            evid.append(EvidenceDir(p))
        except Exception as exc:  # 任何解析异常都 fail closed
            lines.append(f"[目录] {os.path.basename(p)}:解析异常 {type(exc).__name__}: {exc}")
            evid.append(None)
    # ★r4:跨目录/目录内独立性(时间窗重叠 = 克隆目录或复制记录)。
    # 在目录报告前执行:涉事目录按既有不合格路径报告并使总体 FAIL,
    # evaluate_gates 亦跳过 issue 目录(样本计数随之失去支撑)。
    cross_dir_independence(evid)

    ok = True
    hand_filled = 0
    for d in evid:
        if d is None:
            ok = False
            continue
        hand_filled += d.hand_filled
        cfg = "/".join([d.config[0], f"mux={int(d.config[1])}", d.config[2]]) if d.config else "?"
        status = "OK" if not d.issues else f"不合格({len(d.issues)} 项)"
        lines.append(f"[目录] {d.name} ({cfg}, run_id={d.run_id}):{status}")
        for msg in d.issues:
            lines.append(f"  ✗ {msg}")
        if d.issues:
            ok = False
    if hand_filled:
        lines.append(f"(忽略记录中手填判定字段 {hand_filled} 处:核验只认原始证据)")

    good_dirs = [d for d in evid if d is not None and not d.issues]

    # r3:报告实际核验过的源快照(离线证据的追溯锚点)
    for d in good_dirs:
        if d.build_binding:
            lines.append(f"[build] {d.name} 源绑定已核验:{d.build_binding}")
            break   # 跨目录 manifest 一致性另查;绑定描述相同,打一条即可

    # ★r2:跨目录构建一致性(同一份源/二进制,不许不同构建拼装)
    if len(good_dirs) > 1:
        bin_sets = {tuple(sorted(d.binaries_hashes.items())) for d in good_dirs}
        src_sets = {d.sources_manifest for d in good_dirs}
        if len(bin_sets) > 1:
            ok = False
            lines.append("✗ 跨目录构建不一致:binaries.json 哈希不同(不同二进制拼装,全部门槛作废)")
        if len(src_sets) > 1:
            ok = False
            lines.append("✗ 跨目录构建不一致:sources.sha256 不同(不同源码版本拼装,全部门槛作废)")
        if len(bin_sets) == 1 and len(src_sets) == 1:
            lines.append("跨目录构建一致:同一份源/二进制(6 协议证据同源)")

    gates = evaluate_gates([d for d in evid if d is not None])
    lines.append("")
    for g in gates:
        lines.append(f"[{'PASS' if g.ok else 'FAIL'}] {g.gid} {g.title}")
        for det in g.details:
            lines.append(f"      {det}")
        if not g.ok:
            ok = False
    lines.append("")
    lines.append("总体:" + ("PASS" if ok else "FAIL"))
    return ok, "\n".join(lines)


# ---------------------------------------------------------------------------
# 自检(--self-test):用合成证据验证核验器 fail closed
# ---------------------------------------------------------------------------

FIX_BASE_US = 1_800_000_000_000_000
_SYNTH_BIN = {"rh-server": b"synthetic-rh-server-v1", "rh-client": b"synthetic-rh-client-v1"}
_SYNTH_SRC = [
    ("Cargo.toml", b"[workspace]\nmembers = [\"crates/synthetic\"]\n"),
    ("crates/synthetic/Cargo.toml", b"[package]\nname = \"synthetic\"\n"),
    ("crates/synthetic/a.rs", b"// synthetic source a\n" * 64),
    ("vendor/synthetic/b.rs", b"// synthetic source b\n" * 64),
]


def _fx_iperf_json(mbps, *, streams, reverse, duration, ts_us, dst, dst_port):
    """与真实 iperf3 -J 输出同构的合成 raw(含 start 元数据)。

    ★r3:timestamp 用真实标准对象形态 {"time","timesecs"[,"timemillisecs"]}
    (实测 iperf3 3.22 如此;3.17.1 同族 schema,验收机现场再核对)。"""
    return {
        "start": {
            "connected": [{"socket": 4, "local_host": "10.9.9.9", "local_port": 51000,
                           "remote_host": dst, "remote_port": dst_port}],
            "test_start": {"protocol": "TCP", "num_streams": streams,
                           "reverse": reverse, "duration": duration,
                           "blksize": 131072, "omit": 0},
            "timestamp": {"time": "Tue, 13 Oct 2026 00:00:00 UTC",
                          "timesecs": ts_us // 1_000_000,
                          "timemillisecs": ts_us // 1_000},
        },
        "end": {"sum_received": {"bits_per_second": int(mbps * 1e6 + 0.5),
                                 "seconds": duration}},
    }


def _fx_repo(root):
    """合成源码树(像 Cargo 工作区:根/子 crate 的 Cargo.toml + rs 源)。"""
    repo = os.path.join(root, "fx-repo")
    for rel, content in _SYNTH_SRC:
        fp = os.path.join(repo, rel)
        os.makedirs(os.path.dirname(fp), exist_ok=True)
        with open(fp, "wb") as fh:
            fh.write(content)
    return repo


def _fx_manifest(repo):
    """清单内容 = 共享枚举规则 + 真实哈希(与 build_manifest.py 同一实现)。"""
    lines = []
    for rel in source_inventory.enumerate_source_files(repo):
        fp = os.path.join(repo, rel)
        lines.append(hashlib.sha256(open(fp, "rb").read()).hexdigest() + "  " + rel)
    return "\n".join(lines) + "\n"


def build_fixture_dir(root, name, proto, mux, relay, samples=3, *,
                      tunnel_mbps=None, direct_mbps=10000.0,
                      duration_s=2.0, cpu_pct=30.0, cpu_hz=20,
                      concurrent=True, udp_loss=0.2, label=None,
                      g5_verified=None, base_us=None):
    """构造一个自洽的合成证据目录(默认全部门槛可过)。"""
    d = os.path.join(root, name)
    os.makedirs(os.path.join(d, "endpoints", "server"), exist_ok=True)
    os.makedirs(os.path.join(d, "endpoints", "client"), exist_ok=True)
    os.makedirs(os.path.join(d, "raw"), exist_ok=True)
    os.makedirs(os.path.join(d, "build", "bin"), exist_ok=True)
    label = label or name

    repo = _fx_repo(root)
    bin_sha = {b: hashlib.sha256(payload).hexdigest()
               for b, payload in _SYNTH_BIN.items()}
    for b, payload in _SYNTH_BIN.items():
        with open(os.path.join(d, "build", "bin", b), "wb") as fh:
            fh.write(payload)
    with open(os.path.join(d, "build", "binaries.json"), "w", encoding="utf-8") as fh:
        json.dump({"repo_root": repo,
                   "binaries": {b: {"sha256": bin_sha[b], "path": f"/synthetic/{b}"}
                                for b in ("rh-server", "rh-client")}}, fh)
    with open(os.path.join(d, "build", "sources.sha256"), "w", encoding="utf-8") as fh:
        fh.write(_fx_manifest(repo))

    if tunnel_mbps is None:
        tunnel_mbps = {"tcp": 9000.0, "tls": 2600.0}.get(proto, 9000.0)
    mux_s = "true" if mux else "false"
    run_id = f"fx-{name}"

    with open(os.path.join(d, "schema.json"), "w", encoding="utf-8") as fh:
        json.dump({"schema": SCHEMA, "version": SCHEMA_VERSION, "run_id": run_id}, fh)

    # ---- 端点证据 ----
    for role in ("server", "client"):
        ep = {
            "role": role, "run_id": run_id,
            "config": {"proto": proto, "multiplex": mux_s, "relay_mode": relay},
            "env": {"RH_NOTSENT_LOWAT": "unset", "RH_QUIC_CC": "unset"},
            "identity": {"pid": 123,
                         "exe_sha256": bin_sha["rh-server" if role == "server" else "rh-client"],
                         "version": "fx", "source": "proc_exe"},
            "nofile": {"soft": "65535", "hard": "unlimited", "ok": True},
        }
        if role == "client":
            ep["iperf3"] = "iperf 3.17.1 (synthetic)"
        with open(os.path.join(d, "endpoints", role, "endpoint.json"), "w",
                  encoding="utf-8") as fh:
            json.dump(ep, fh)
        with open(os.path.join(d, "endpoints", role, "identity.jsonl"), "w",
                  encoding="utf-8") as fh:
            fh.write(json.dumps({"name": f"rh-{role}", "pid": 123,
                                 "exe_sha256": ep["identity"]["exe_sha256"],
                                 "version": "fx", "source": "proc_exe"}) + "\n")

    # ---- 时间线:吞吐窗口(每个 duration_s)→ UDP → 并发窗口 ----
    recs = []
    t = FIX_BASE_US if base_us is None else base_us
    windows = []           # 需被 CPU 覆盖的 (w0,w1)
    for sample in range(1, samples + 1):
        for streams in STREAM_TIERS:
            for direction in DIRECTIONS:
                for path in ("direct", "tunnel"):
                    w0 = t
                    w1 = t + int(duration_s * 1e6)
                    t = w1
                    mbps = direct_mbps if path == "direct" else tunnel_mbps
                    dst = "10.0.0.2" if path == "direct" else "10.0.0.1"
                    dst_port = 19101 if path == "direct" else 20002
                    rid = f"thp-{label}-s{sample}-{path}-{direction}-{streams}s"
                    with open(os.path.join(d, "raw", rid + ".json"), "w",
                              encoding="utf-8") as fh:
                        json.dump(_fx_iperf_json(
                            mbps, streams=streams,
                            reverse=1 if direction == "download" else 0,
                            duration=duration_s, ts_us=w0,
                            dst=dst, dst_port=dst_port), fh)
                    with open(os.path.join(d, "raw", rid + ".stderr"), "w",
                              encoding="utf-8") as fh:
                        fh.write("")
                    with open(os.path.join(d, "raw", rid + ".exit"), "w",
                              encoding="utf-8") as fh:
                        fh.write("0\n")
                    recs.append({
                        "metric": "throughput", "valid": True, "mbps": int(mbps),
                        "exit": 0, "window_start_us": w0, "window_end_us": w1,
                        "raw_json": f"raw/{rid}.json", "raw_stderr": f"raw/{rid}.stderr",
                        "raw_exit": f"raw/{rid}.exit",
                        "path": path, "dir": direction, "streams": streams,
                        "sample": sample, "dst": dst, "dst_port": dst_port,
                        "pair": f"s{sample}-{direction}-{streams}s",
                        "proto": proto, "relay": relay, "mux": mux_s,
                        "run_id": run_id, "label": label,
                    })
                    if path == "tunnel":
                        windows.append((w0, w1))

    # ---- UDP(排空口径字段齐全;packet_size 固定 1200)----
    for sample in range(1, samples + 1):
        for path in ("direct", "tunnel"):
            sent, late, dup = 1000, 2, 1
            uniq = 1000 - int(round(udp_loss * 10)) + late
            lost = sent - uniq
            data = {
                "sent": sent, "received": uniq - late, "late_recv": late, "dup": dup,
                "corrupt": 0, "recv_unique": uniq, "lost": lost,
                "loss_pct": round(lost * 100.0 / sent, 3),
                "packet_size": 1200, "seconds": 8.0, "drain_seconds": 5.0,
                "mbps": 1.4, "pps": 150.0,
                # ★r4:独立测量窗口串行不重叠(与 peer.sh 顺序执行一致)
                "started_us": t, "ended_us": t + 8_000_000,
            }
            t += 8_000_000
            recs.append({"metric": "udp", "path": path, "proto": proto, "relay": relay,
                         "mux": mux_s, "run_id": run_id, "label": label,
                         "sample": sample, "data": data})

    # ---- 并发(六协议目录都带,verified=4096)----
    if concurrent:
        for sample in range(1, samples + 1):
            verified = 4096 if g5_verified is None else g5_verified
            data = {
                "metric": "concurrent", "path": "tunnel", "proto": proto, "relay": relay,
                "mux": mux_s, "run_id": run_id, "label": label, "n": 4096,
                "sample": sample,
                "established": 4096, "initial_verified": verified,
                "hold_alive": verified, "verified": verified,
                "failed": 4096 - verified,
                "connect_failed": 0, "verify_failed": 0, "hold_lost": 4096 - verified,
                "hold_seconds": 5, "hold_rounds": 5, "alive_by_round": [verified] * 5,
                "setup_seconds": 3.0, "connect_rate_per_s": 1365.3,
                "started_us": t, "ended_us": t + 20_000_000,
            }
            recs.append(data)
            windows.append((t, t + 20_000_000))
            t += 20_000_000

    # ---- meta ----
    # ★r5:started_at 从本目录时间线基点推导(与绝对时间锚一致)
    import datetime as _dt
    started_at = _dt.datetime.fromtimestamp(
        (FIX_BASE_US if base_us is None else base_us) / 1e6,
        tz=_dt.timezone.utc).isoformat()
    meta = {
        "metric": "meta", "schema_version": SCHEMA_VERSION, "run_id": run_id,
        "label": label, "proto": proto, "relay": relay, "mux": mux_s,
        "client_version": "fx-client-1.0", "drive_iperf3": "iperf 3.17.1 (fx)",
        "drive_nofile_soft": "65535", "peer_duration_s": duration_s, "repeat": samples,
        "conc_levels": [64, 256, 1024, 4096],
        "cc_source": "endpoints", "lowat_source": "endpoints",
        "config_evidence": {"server": "endpoints/server/endpoint.json",
                            "client": "endpoints/client/endpoint.json"},
        "started_at": started_at,
    }
    with open(os.path.join(d, f"results-{label}-fx.jsonl"), "w", encoding="utf-8") as fh:
        for r in [meta] + recs:
            fh.write(json.dumps(r) + "\n")

    # ---- CPU 时间线:覆盖全部窗口,ticks 按 cpu_pct 增长 ----
    if windows:
        t0_cpu = windows[0][0] - 500_000
        t1_cpu = windows[-1][1] + 500_000
    else:
        t0_cpu, t1_cpu = t, t + 10_000_000
    clk = 100
    step_us = int(1e6 / cpu_hz)
    for role in ("server", "client"):
        with open(os.path.join(d, "endpoints", role, "cpu.jsonl"), "w",
                  encoding="utf-8") as fh:
            fh.write(json.dumps({"clk_tck": clk, "hz": cpu_hz, "pid": 123}) + "\n")
            tick = 0
            tt = t0_cpu
            while tt <= t1_cpu:
                fh.write(json.dumps({"t_us": tt, "ticks": tick,
                                     "rss_kb": 48 * 1024}) + "\n")
                tick += max(1, int(cpu_pct / 100.0 * clk / cpu_hz))
                tt += step_us
    return d


def build_good_set(root, samples=3):
    """构造完整可过的证据集,返回父目录路径。

    tls 直连基线 3000(tunnel 2600 → 保留率 86.7%):正向对照必须是真的能过,
    而不是碰巧没被检出的样本。★r5:各目录时间线串行错开(换协议须重启
    两端串行执行,跨配置窗口互斥)。"""
    for i, proto in enumerate(PROTOS):
        direct = 3000.0 if proto == "tls" else 10000.0
        build_fixture_dir(root, f"{proto}-mux", proto, True, "splice",
                          samples=samples, direct_mbps=direct,
                          base_us=FIX_BASE_US + i * 3_600_000_000)
    build_fixture_dir(root, "tcp-nomux", "tcp", False, "splice",
                      samples=samples, tunnel_mbps=3500.0,
                      base_us=FIX_BASE_US + len(PROTOS) * 3_600_000_000)
    return root


def glob2_results(d):
    import glob as _g
    return sorted(_g.glob(os.path.join(d, "results-*.jsonl")))


def run_self_test():
    """对抗性自检:好证据必须 PASS;每种"旧口径/缺证据/伪造证据"必须 FAIL。"""
    results = []

    def expect(name, ok_expected, reason_substr, fn):
        with tempfile.TemporaryDirectory(prefix="rh-verify-goal-") as tmp:
            try:
                path = fn(tmp)
                got_ok, report = verify_path(path)
            except Exception as exc:
                results.append((name, False,
                                f"异常 {type(exc).__name__}: {exc}(期望 "
                                f"{'PASS' if ok_expected else 'FAIL(' + reason_substr + ')'})"))
                return
            if got_ok != ok_expected:
                results.append((name, False,
                                f"期望 {'PASS' if ok_expected else 'FAIL'},得到 "
                                f"{'PASS' if got_ok else 'FAIL'}\n--- 报告 ---\n{report}"))
                return
            if reason_substr and reason_substr not in report:
                results.append((name, False,
                                f"期望报告含 {reason_substr!r}:\n{report}"))
                return
            results.append((name, True, ""))

    # 0) 正向对照:核验器不是"永远 FAIL"
    expect("good-set 全门槛 PASS", True, "总体:PASS", build_good_set)

    # ---- 主代理复现的 6 个误通过负例(r2 全部修为 fail closed)----

    def wrong_udp_size(tmp):
        build_good_set(tmp)

        def edit(path):
            rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            for row in rows:
                if row.get("metric") == "udp":
                    row["data"]["packet_size"] = 1
            with open(path, "w", encoding="utf-8") as fh:
                for r in rows:
                    fh.write(json.dumps(r) + "\n")
        import glob
        for path in glob.glob(os.path.join(tmp, "*", "results-*.jsonl")):
            edit(path)
        return tmp
    expect("UDP packet_size=1 ≠1200 → FAIL", False, "1200", wrong_udp_size)

    def missing_four_proto_conc(tmp):
        build_good_set(tmp)
        import glob
        for path in glob.glob(os.path.join(tmp, "*", "results-*.jsonl")):
            dname = os.path.basename(os.path.dirname(path))
            if dname in ("ws-mux", "wss-mux", "quic-mux", "kcp-mux"):
                rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
                rows = [r for r in rows if r.get("metric") != "concurrent"]
                with open(path, "w", encoding="utf-8") as fh:
                    for r in rows:
                        fh.write(json.dumps(r) + "\n")
        return tmp
    expect("删除四协议并发记录 → G5 FAIL", False, "G5", missing_four_proto_conc)

    def fabricated_source(tmp):
        build_good_set(tmp)
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "build", "sources.sha256")):
            with open(p, "w", encoding="utf-8") as fh:
                fh.write("a" * 64 + "  missing-current-source.rs\n")
        return tmp
    expect("伪造 source manifest(文件不存在)→ FAIL", False,
           "missing-current-source.rs", fabricated_source)

    def no_hold_proof(tmp):
        build_good_set(tmp)
        import glob
        for path in glob.glob(os.path.join(tmp, "*", "results-*.jsonl")):
            rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            for r in rows:
                if r.get("metric") == "concurrent":
                    for field in ("hold_alive", "initial_verified",
                                  "hold_rounds", "alive_by_round"):
                        r.pop(field, None)
            with open(path, "w", encoding="utf-8") as fh:
                for r in rows:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("并发只留 verified/established(无保持证据)→ FAIL", False,
           "concurrent 记录缺", no_hold_proof)

    def wrong_raw_shape(tmp):
        build_good_set(tmp)
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "raw", "*.json")):
            doc = json.loads(open(p, encoding="utf-8").read())
            doc["start"] = {"test_start": {"protocol": "UDP", "reverse": 1,
                                           "num_streams": 99}}
            with open(p, "w", encoding="utf-8") as fh:
                json.dump(doc, fh)
        return tmp
    expect("raw 测试形态错配(UDP/reverse/99流)→ FAIL", False,
           "test_start.protocol", wrong_raw_shape)

    def insufficient_nofile(tmp):
        build_good_set(tmp)
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "endpoints", "*", "endpoint.json")):
            doc = json.loads(open(p, encoding="utf-8").read())
            doc["nofile"] = {"soft": "64", "hard": "64", "ok": False}
            with open(p, "w", encoding="utf-8") as fh:
                json.dump(doc, fh)
        for path in glob.glob(os.path.join(tmp, "*", "results-*.jsonl")):
            rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            for row in rows:
                if row.get("metric") == "meta":
                    row["drive_nofile_soft"] = "64"
            with open(path, "w", encoding="utf-8") as fh:
                for r in rows:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("端点+drive nofile=64 → 环境错误 FAIL", False,
           "fd 软限", insufficient_nofile)

    # ---- r4:独立性/克隆/混装负例(独立审查二轮复现的绕过)----

    def clone_dir_bypass(tmp):
        # samples=1 基线因 G7 配对<3 FAIL;整目录克隆 2 份冒充 3 次独立测量
        # (克隆满足跨目录构建一致检查 → 修前可全过)
        import shutil
        build_good_set(tmp, samples=1)
        for name in sorted(os.listdir(tmp)):
            full = os.path.join(tmp, name)
            if name == "fx-repo" or not os.path.isdir(full):
                continue
            for k in (2, 3):
                shutil.copytree(full, os.path.join(tmp, f"{name}-clone{k}"))
        return tmp
    expect("整目录克隆冒充多次独立测量 → FAIL", False,
           "独立性被破坏", clone_dir_bypass)

    def dup_udp_window(tmp):
        # 唯一 UDP tunnel 记录复制 2 份:改 sample 号、时间窗原样保留
        # (同目录复制,修前计入 G4 样本数)
        import glob
        build_good_set(tmp)
        path = glob.glob(os.path.join(tmp, "tls-mux", "results-*.jsonl"))[0]
        rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
        out = []
        for r in rows:
            out.append(r)
            if r.get("metric") == "udp" and r.get("path") == "tunnel":
                for k, new_sample in ((1, 4), (2, 5)):
                    c = json.loads(json.dumps(r))
                    c["sample"] = new_sample
                    out.append(c)
        with open(path, "w", encoding="utf-8") as fh:
            for r in out:
                fh.write(json.dumps(r) + "\n")
        return tmp
    expect("UDP 记录复制(改号保时间窗)→ FAIL", False, "时间窗重叠", dup_udp_window)

    def dup_concurrent_window(tmp):
        # 唯一 concurrent 记录复制 2 份:改 sample 号、时间窗原样保留
        import glob
        build_good_set(tmp)
        path = glob.glob(os.path.join(tmp, "tls-mux", "results-*.jsonl"))[0]
        rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
        out = []
        for r in rows:
            out.append(r)
            if r.get("metric") == "concurrent":
                for new_sample in (4, 5):
                    c = json.loads(json.dumps(r))
                    c["sample"] = new_sample
                    out.append(c)
        with open(path, "w", encoding="utf-8") as fh:
            for r in out:
                fh.write(json.dumps(r) + "\n")
        return tmp
    expect("concurrent 记录复制(改号保时间窗)→ FAIL", False,
           "时间窗重叠", dup_concurrent_window)

    def schema_run_id_mismatch(tmp):
        # schema.json 来自另一轮(run_id 不一致)与 results 混装
        build_good_set(tmp)
        p = os.path.join(tmp, "tcp-nomux", "schema.json")
        doc = json.loads(open(p, encoding="utf-8").read())
        doc["run_id"] = "fx-other-round"
        with open(p, "w", encoding="utf-8") as fh:
            json.dump(doc, fh)
        return tmp
    expect("schema run_id 与 meta 不一致(混装)→ FAIL", False,
           "目录混装", schema_run_id_mismatch)

    def udp_missing_window(tmp):
        # 旧口径 UDP:无 started_us/ended_us(无法做独立性查重)
        import glob
        build_good_set(tmp)
        for path in glob.glob(os.path.join(tmp, "*", "results-*.jsonl")):
            rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            changed = False
            for r in rows:
                if r.get("metric") == "udp":
                    r["data"].pop("started_us", None)
                    r["data"].pop("ended_us", None)
                    changed = True
            if changed:
                with open(path, "w", encoding="utf-8") as fh:
                    for r in rows:
                        fh.write(json.dumps(r) + "\n")
        return tmp
    expect("UDP 缺时间窗字段(无法查独立性)→ FAIL", False,
           "started_us", udp_missing_window)

    # ---- r5 负例:克隆 + 全局时间平移(独立审查三轮实验 D 的残余绕过)----

    def _shift_dir_time(d, delta_us):
        """把一个证据目录的全部时间字段机械平移(窗口/raw 时间戳/cpu 采样)。"""
        import glob
        import re as _re
        # results jsonl:窗口字段 + udp/concurrent started/ended_us
        for path in glob.glob(os.path.join(d, "results-*.jsonl")):
            rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            for r in rows:
                data = r.get("data", r)
                for f in ("window_start_us", "window_end_us",
                          "started_us", "ended_us"):
                    if f in data and is_num(data[f]):
                        data[f] += delta_us
                    elif f in r and is_num(r[f]):
                        r[f] += delta_us
            with open(path, "w", encoding="utf-8") as fh:
                for r in rows:
                    fh.write(json.dumps(r) + "\n")
        # raw iperf JSON:start.timestamp(对象形态,timesecs/timemillisecs)
        for path in glob.glob(os.path.join(d, "raw", "*.json")):
            doc = json.loads(open(path, encoding="utf-8").read())
            ts = ((doc.get("start") or {}).get("timestamp")) or {}
            if isinstance(ts.get("timemillisecs"), int):
                ts["timemillisecs"] += delta_us // 1000
            if isinstance(ts.get("timesecs"), int):
                ts["timesecs"] += delta_us // 1_000_000
            with open(path, "w", encoding="utf-8") as fh:
                json.dump(doc, fh)
        # cpu.jsonl:t_us
        for path in glob.glob(os.path.join(d, "endpoints", "*", "cpu.jsonl")):
            lines = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            for s in lines:
                if is_num(s.get("t_us")):
                    s["t_us"] += delta_us
            with open(path, "w", encoding="utf-8") as fh:
                for s in lines:
                    fh.write(json.dumps(s) + "\n")

    def clone_shift_same_rid(tmp):
        # 实验 D:每目录克隆 2 份并整体平移 +24h/+48h,run_id 原样保留
        import shutil
        build_good_set(tmp, samples=1)
        for name in sorted(os.listdir(tmp)):
            full = os.path.join(tmp, name)
            if name == "fx-repo" or not os.path.isdir(full):
                continue
            for k, delta in ((2, 24 * 3600 * 1_000_000),
                             (3, 48 * 3600 * 1_000_000)):
                dst = os.path.join(tmp, f"{name}-clone{k}")
                shutil.copytree(full, dst)
                _shift_dir_time(dst, delta)
        return tmp
    expect("克隆+全局平移(+24/48h,锚外)→ FAIL", False,
           "绝对时间越界", clone_shift_same_rid)

    def clone_shift_within_anchor(tmp):
        # 平移量在 6h 锚窗内(+20min,不与兄弟目录重叠):只有 run_id 全局
        # 互异能抓住(克隆保留原 run_id)
        import shutil
        build_good_set(tmp, samples=1)
        for name in sorted(os.listdir(tmp)):
            full = os.path.join(tmp, name)
            if name == "fx-repo" or not os.path.isdir(full):
                continue
            dst = os.path.join(tmp, f"{name}-clone2")
            shutil.copytree(full, dst)
            _shift_dir_time(dst, 20 * 60 * 1_000_000)
        return tmp
    expect("克隆+锚窗内平移(保 run_id)→ FAIL", False,
           "run_id 重复", clone_shift_within_anchor)

    def clone_shift_new_rid(tmp):
        # 实验 D 强化:克隆+平移,且 run_id 全部改写(schema/results/endpoints
        # 一致)——只有 meta.started_at 未随之平移,绝对时间锚必须抓住
        import shutil
        build_good_set(tmp, samples=1)
        for name in sorted(os.listdir(tmp)):
            full = os.path.join(tmp, name)
            if name == "fx-repo" or not os.path.isdir(full):
                continue
            for k, delta in ((2, 24 * 3600 * 1_000_000),
                             (3, 48 * 3600 * 1_000_000)):
                dst = os.path.join(tmp, f"{name}-clone{k}")
                shutil.copytree(full, dst)
                _shift_dir_time(dst, delta)
                new_rid = f"fx-{name}-c{k}"
                sp = os.path.join(dst, "schema.json")
                doc = json.loads(open(sp, encoding="utf-8").read())
                doc["run_id"] = new_rid
                with open(sp, "w", encoding="utf-8") as fh:
                    json.dump(doc, fh)
                for path in glob2_results(dst):
                    rows = [json.loads(x) for x in open(path, encoding="utf-8")
                            if x.strip()]
                    for r in rows:
                        r["run_id"] = new_rid
                    with open(path, "w", encoding="utf-8") as fh:
                        for r in rows:
                            fh.write(json.dumps(r) + "\n")
                for ep in ("server", "client"):
                    epath = os.path.join(dst, "endpoints", ep, "endpoint.json")
                    doc = json.loads(open(epath, encoding="utf-8").read())
                    doc["run_id"] = new_rid
                    with open(epath, "w", encoding="utf-8") as fh:
                        json.dump(doc, fh)
        return tmp
    expect("克隆+平移+改写 run_id(started_at 锚)→ FAIL", False,
           "绝对时间越界", clone_shift_new_rid)

    def started_at_missing(tmp):
        build_good_set(tmp)
        import glob
        for path in glob.glob(os.path.join(tmp, "*", "results-*.jsonl")):
            rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
            for r in rows:
                if r.get("metric") == "meta":
                    r.pop("started_at", None)
            with open(path, "w", encoding="utf-8") as fh:
                for r in rows:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("meta.started_at 缺失(无法锚定)→ FAIL", False,
           "started_at", started_at_missing)

    def started_at_redated(tmp):
        # started_at 被改写到窗口之后(晚于全部测量)→ FAIL
        import glob
        import datetime as _dt
        build_good_set(tmp)
        victim = os.path.join(tmp, "tcp-nomux")
        path = glob.glob(os.path.join(victim, "results-*.jsonl"))[0]
        rows = [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]
        for r in rows:
            if r.get("metric") == "meta":
                r["started_at"] = _dt.datetime.fromtimestamp(
                    (FIX_BASE_US + 99 * 3_600_000_000) / 1e6,
                    tz=_dt.timezone.utc).isoformat()
        with open(path, "w", encoding="utf-8") as fh:
            for r in rows:
                fh.write(json.dumps(r) + "\n")
        return tmp
    expect("started_at 晚于窗口(锚点置后)→ FAIL", False,
           "绝对时间越界", started_at_redated)

    # ---- 原有对抗矩阵(继续有效)----

    def no_config(tmp):
        build_good_set(tmp)
        os.remove(os.path.join(tmp, "tcp-nomux", "endpoints", "server", "endpoint.json"))
        return tmp
    expect("缺少端点配置证据 → FAIL", False, "endpoints/server", no_config)

    def no_raw(tmp):
        build_good_set(tmp)
        p = os.path.join(tmp, "tcp-nomux", "raw")
        victim = [f for f in os.listdir(p) if "tunnel" in f and "-1s." in f][0]
        os.remove(os.path.join(p, victim))
        return tmp
    expect("缺原始 iperf JSON → FAIL", False, "raw_json", no_raw)

    def bad_cpu(tmp):
        build_good_set(tmp)
        p = os.path.join(tmp, "tcp-nomux", "endpoints", "client", "cpu.jsonl")
        lines = open(p, encoding="utf-8").read().splitlines()
        with open(p, "w", encoding="utf-8") as fh:
            fh.write("\n".join(lines[:6]) + "\n")
        return tmp
    expect("CPU 覆盖率不足 → G1 FAIL", False, "覆盖率", bad_cpu)

    def cpu_per_direction_leak(tmp):
        """上行 CPU 正常、下行 CPU 拉满:方向分别判定必须抓住(不因上行均值掩盖)。

        注意门槛口径是"≥3 次独立配对测量的中位数"(G7),所以只拉爆一个样本
        不足以触发;这里把全部下行单流样本的窗口都拉到 ~80% 单核。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        recs_path = [x for x in os.listdir(d) if x.startswith("results-")][0]
        recs = [json.loads(x) for x in open(os.path.join(d, recs_path), encoding="utf-8")
                if x.strip()]
        dl_wins = [(r["window_start_us"], r["window_end_us"]) for r in recs
                   if (r.get("metric") == "throughput" and r.get("dir") == "download"
                       and r.get("path") == "tunnel" and r.get("streams") == 1)]
        for role in ("server", "client"):
            p = os.path.join(d, "endpoints", role, "cpu.jsonl")
            lines = [json.loads(x) for x in open(p, encoding="utf-8") if x.strip()]
            header, samples = lines[0], lines[1:]
            with open(p, "w", encoding="utf-8") as fh:
                fh.write(json.dumps(header) + "\n")
                for s in samples:
                    if any(w0 <= s["t_us"] <= w1 for w0, w1 in dl_wins):
                        s["ticks"] = s["ticks"] * 4   # ~80% 单核
                    fh.write(json.dumps(s) + "\n")
        return tmp
    expect("下行 CPU 超 50%(方向分别判定)→ G1 FAIL", False, "download", cpu_per_direction_leak)

    def missing_proto(tmp):
        for proto in PROTOS:
            if proto == "ws":
                continue
            build_fixture_dir(tmp, f"{proto}-mux", proto, True, "splice")
        build_fixture_dir(tmp, "tcp-nomux", "tcp", False, "splice", tunnel_mbps=3500.0)
        return tmp
    expect("缺 ws 协议证据 → G3 FAIL", False, "ws", missing_proto)

    def missing_dir(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        f = [x for x in os.listdir(d) if x.startswith("results-")][0]
        fp = os.path.join(d, f)
        recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
        keep = [r for r in recs
                if not (r.get("metric") == "throughput" and r.get("dir") == "download")]
        with open(fp, "w", encoding="utf-8") as fh:
            for r in keep:
                fh.write(json.dumps(r) + "\n")
        return tmp
    expect("缺 download 方向 → G1 FAIL", False, "download", missing_dir)

    def below_g1_mut(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                if r.get("metric") == "throughput" and r.get("dir") == "upload" \
                        and r.get("streams") == 1:
                    r["mbps"] = 2800
                    raw_path = os.path.join(d, r["raw_json"])
                    raw = json.loads(open(raw_path, encoding="utf-8").read())
                    raw["end"]["sum_received"]["bits_per_second"] = int(2800 * 1e6)
                    with open(raw_path, "w", encoding="utf-8") as fh:
                        json.dump(raw, fh)
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("G1 单流 2800 < 3000 → FAIL", False, "G1", below_g1_mut)

    def invalid_baseline(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                if r.get("metric") == "throughput" and r.get("path") == "direct" \
                        and r.get("streams") == 1 and r.get("dir") == "upload":
                    r["valid"] = False
                    r["mbps"] = None
                    r["exit"] = 1
                    r["error"] = {"iperf_exit": 1, "stderr_tail": "synthetic failure"}
                    rid = r["raw_exit"].split("/")[-1]
                    with open(os.path.join(d, "raw", rid), "w", encoding="utf-8") as fh:
                        fh.write("1\n")
                    break   # 只毁一份,使有效配对掉到 2 < 3
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("直连基线无效 → 配对不足 FAIL", False, "< 3", invalid_baseline)

    def handshake_only(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tls-mux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                if r.get("metric") == "concurrent":
                    for field in ("verified", "initial_verified", "hold_alive",
                                  "hold_rounds", "alive_by_round"):
                        r.pop(field, None)
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("仅握手并发 → G5 FAIL", False, "verified", handshake_only)

    def fake_hold_numbers(tmp):
        """verified=4096 但 alive_by_round 只有 1:保持结论与逐轮证据矛盾。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "tls-mux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                if r.get("metric") == "concurrent":
                    r["alive_by_round"] = [1] * r.get("hold_rounds", 5)
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("伪造保持(alive_by_round=1 与 verified=4096 矛盾)→ FAIL", False,
           "矛盾", fake_hold_numbers)

    expect("仅 2 次配对 → G7 FAIL", False, "< 3",
           lambda tmp: build_good_set(tmp, samples=2))

    def duplicated_sample(tmp):
        """同 key 三份相同时间窗的样本冒充三次独立测量。

        克隆连 raw 文件一起复制(文件名/内容全对得上),唯一破绽是三条记录
        的时间窗完全相同 —— 独立测量不可能时钟不动。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        f = [x for x in os.listdir(d) if x.startswith("results-")][0]
        fp = os.path.join(d, f)
        recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
        src = None
        for r in recs:
            if (r.get("metric") == "throughput" and r.get("dir") == "upload"
                    and r.get("streams") == 1 and r.get("path") == "tunnel"
                    and r.get("sample") == 1):
                src = dict(r)
                break
        clones = []
        for sample in (2, 3):
            clone = dict(src)
            clone["sample"] = sample
            for field in ("raw_json", "raw_stderr", "raw_exit"):
                src_name = os.path.basename(src[field])
                dst_name = src_name.replace("-s1-", f"-s{sample}-")
                with open(os.path.join(d, src[field]), "rb") as fh_in, \
                        open(os.path.join(d, "raw", dst_name), "wb") as fh_out:
                    fh_out.write(fh_in.read())
                clone[field] = f"raw/{dst_name}"
            clones.append(clone)
        recs = [r for r in recs if not (r.get("metric") == "throughput"
                                        and r.get("dir") == "upload"
                                        and r.get("streams") == 1
                                        and r.get("path") == "tunnel"
                                        and r.get("sample") in (2, 3))]
        recs.extend(clones)
        with open(fp, "w", encoding="utf-8") as fh:
            for r in recs:
                fh.write(json.dumps(r) + "\n")
        return tmp
    expect("重复时间窗样本冒充独立测量 → FAIL", False, "重复", duplicated_sample)

    def hand_filled(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                r["pass"] = True
                r["verdict"] = "pass"
                if r.get("metric") == "throughput" and r.get("dir") == "upload" \
                        and r.get("streams") == 1 and r.get("path") == "tunnel":
                    r["mbps"] = 100
                    raw_path = os.path.join(d, r["raw_json"])
                    raw = json.loads(open(raw_path, encoding="utf-8").read())
                    raw["end"]["sum_received"]["bits_per_second"] = int(100 * 1e6)
                    with open(raw_path, "w", encoding="utf-8") as fh:
                        json.dump(raw, fh)
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("手填 pass 字段被忽略 → 仍 FAIL", False, "G1", hand_filled)

    def binary_mismatch(tmp):
        build_good_set(tmp)
        p = os.path.join(tmp, "tcp-nomux", "endpoints", "client", "endpoint.json")
        ep = json.load(open(p, encoding="utf-8"))
        ep["identity"]["exe_sha256"] = "0" * 64
        json.dump(ep, open(p, "w", encoding="utf-8"))
        return tmp
    expect("运行二进制哈希不匹配 → FAIL", False, "不一致", binary_mismatch)

    def mixed_builds(tmp):
        """第二个目录整目录改用另一份"构建"(binaries/identity 同步改、无 bin 副本):
        目录自身自洽,唯一破绽是跨目录构建不一致 → 必须 FAIL。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "kcp-mux")
        fake = "f" * 64
        bj = os.path.join(d, "build", "binaries.json")
        doc = json.load(open(bj, encoding="utf-8"))
        doc["binaries"]["rh-server"]["sha256"] = fake
        json.dump(doc, open(bj, "w", encoding="utf-8"))
        ep_path = os.path.join(d, "endpoints", "server", "endpoint.json")
        ep = json.load(open(ep_path, encoding="utf-8"))
        ep["identity"]["exe_sha256"] = fake
        json.dump(ep, open(ep_path, "w", encoding="utf-8"))
        os.remove(os.path.join(d, "build", "bin", "rh-server"))
        return tmp
    expect("跨目录不同二进制拼装 → FAIL", False, "跨目录构建不一致", mixed_builds)

    def nan_mbps(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        f = [x for x in os.listdir(d) if x.startswith("results-")][0]
        fp = os.path.join(d, f)
        text = open(fp, encoding="utf-8").read()
        text = text.replace('"mbps": 3500', '"mbps": NaN', 1)
        if '"mbps": NaN' not in text:      # 兼容 json.dumps 默认分隔符
            text = text.replace('"mbps":3500', '"mbps":NaN', 1)
        with open(fp, "w", encoding="utf-8") as fh:
            fh.write(text)
        return tmp
    expect("NaN 吞吐值 → 坏行 FAIL", False, "NaN", nan_mbps)

    def old_udp(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-mux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                if r.get("metric") == "udp":
                    r["data"] = {"sent": 500, "received": 469, "lost": 31,
                                 "loss_pct": 6.2, "mbps": 0.7, "seconds": 8.0}
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("UDP 旧口径(无排空字段)→ FAIL", False, "旧口径", old_udp)

    def undrained_udp(tmp):
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-mux")
        for f in os.listdir(d):
            if not f.startswith("results-"):
                continue
            fp = os.path.join(d, f)
            recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
            for r in recs:
                if r.get("metric") == "udp":
                    r["data"]["drain_seconds"] = 0
            with open(fp, "w", encoding="utf-8") as fh:
                for r in recs:
                    fh.write(json.dumps(r) + "\n")
        return tmp
    expect("UDP drain_seconds=0(未排空)→ FAIL", False, "drain_seconds", undrained_udp)

    def tls_only(tmp):
        build_fixture_dir(tmp, "tls-mux", "tls", True, "splice")
        return tmp
    expect("仅 TLS 专项 ≠ 全协议门槛 → FAIL", False, "G3", tls_only)

    def no_tcp_control(tmp):
        for proto in PROTOS:
            build_fixture_dir(tmp, f"{proto}-mux", proto, True, "splice")
        return tmp
    expect("G5 缺 tcp 裸连接对照 → FAIL", False, "对照", no_tcp_control)

    def g5_short_mut(tmp):
        build_good_set(tmp)
        for name in ("tls-mux", "tcp-nomux"):
            d = os.path.join(tmp, name)
            for f in os.listdir(d):
                if not f.startswith("results-"):
                    continue
                fp = os.path.join(d, f)
                recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
                for r in recs:
                    if r.get("metric") == "concurrent" and r.get("sample") == 1:
                        r["verified"] = r["initial_verified"] = r["hold_alive"] = 4095
                        r["hold_lost"] = 1
                        r["alive_by_round"] = [4095] * r["hold_rounds"]
                with open(fp, "w", encoding="utf-8") as fh:
                    for r in recs:
                        fh.write(json.dumps(r) + "\n")
        return tmp
    expect("G5 verified=4095 → FAIL", False, "G5", g5_short_mut)

    def raw_timestamp_skew(tmp):
        """raw start.timestamp(标准对象)偏离记录窗口 60s:时间窗错配。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        f = [x for x in os.listdir(d) if x.startswith("results-")][0]
        fp = os.path.join(d, f)
        recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
        for r in recs:
            if r.get("metric") == "throughput" and r.get("path") == "tunnel" \
                    and r.get("streams") == 1 and r.get("dir") == "upload":
                raw_path = os.path.join(d, r["raw_json"])
                raw = json.loads(open(raw_path, encoding="utf-8").read())
                raw["start"]["timestamp"]["timesecs"] += 60
                raw["start"]["timestamp"]["timemillisecs"] += 60_000
                with open(raw_path, "w", encoding="utf-8") as fh:
                    json.dump(raw, fh)
                break
        return tmp
    expect("raw 时间戳偏离窗口 → FAIL", False, "timestamp", raw_timestamp_skew)

    def fake_numeric_timestamp(tmp):
        """timestamp 被强转成伪数值(非标准对象形态):必须 FAIL。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        f = [x for x in os.listdir(d) if x.startswith("results-")][0]
        fp = os.path.join(d, f)
        recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
        for r in recs:
            if r.get("metric") == "throughput" and r.get("path") == "tunnel" \
                    and r.get("streams") == 1 and r.get("dir") == "upload":
                raw_path = os.path.join(d, r["raw_json"])
                raw = json.loads(open(raw_path, encoding="utf-8").read())
                raw["start"]["timestamp"] = raw["start"]["timestamp"]["timesecs"]
                with open(raw_path, "w", encoding="utf-8") as fh:
                    json.dump(raw, fh)
                break
        return tmp
    expect("timestamp 被强转为伪数值 → FAIL", False, "非标准对象形态",
           fake_numeric_timestamp)

    def timesecs_only_ok(tmp):
        """只有 timesecs(无 millis)的合法回退:仍必须 PASS(精度容差)。"""
        build_good_set(tmp)
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "raw", "*.json")):
            raw = json.loads(open(p, encoding="utf-8").read())
            raw["start"]["timestamp"].pop("timemillisecs", None)
            with open(p, "w", encoding="utf-8") as fh:
                json.dump(raw, fh)
        return tmp
    expect("timesecs-only(秒精度)合法 → PASS", True, "总体:PASS", timesecs_only_ok)

    def manifest_omits_source(tmp):
        """repo 里新增源文件但清单不收(主代理 real-schema 负例 B):必须 FAIL。"""
        build_good_set(tmp)
        repo = os.path.join(tmp, "fx-repo")
        victim = os.path.join(repo, "crates", "actual-unlisted-source.rs")
        with open(victim, "w", encoding="utf-8") as fh:
            fh.write("// source changes are not covered by the submitted manifest\n")
        return tmp
    expect("清单遗漏真实源文件 → FAIL", False, "遗漏", manifest_omits_source)

    def manifest_duplicate_entry(tmp):
        build_good_set(tmp)
        import glob
        p = glob.glob(os.path.join(tmp, "tcp-nomux", "build", "sources.sha256"))[0]
        text = open(p, encoding="utf-8").read()
        first = text.splitlines()[0]
        with open(p, "w", encoding="utf-8") as fh:
            fh.write(text + first + "\n")
        return tmp
    expect("清单重复条目 → FAIL", False, "重复条目", manifest_duplicate_entry)

    def manifest_symlink_entry(tmp):
        """清单列入符号链接(逃逸来源):必须 FAIL。"""
        build_good_set(tmp)
        repo = os.path.join(tmp, "fx-repo")
        link = os.path.join(repo, "crates", "synthetic", "link.rs")
        os.symlink(os.path.join(repo, "crates", "synthetic", "a.rs"), link)
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "build", "sources.sha256")):
            text = open(p, encoding="utf-8").read()
            h = hashlib.sha256(open(link, "rb").read()).hexdigest()
            with open(p, "w", encoding="utf-8") as fh:
                fh.write(text + f"{h}  crates/synthetic/link.rs\n")
        return tmp
    expect("清单含符号链接条目 → FAIL", False, "符号链接", manifest_symlink_entry)

    def manifest_extra_entry(tmp):
        """清单包含枚举规则之外的文件(伪造覆盖面):必须 FAIL。"""
        build_good_set(tmp)
        repo = os.path.join(tmp, "fx-repo")
        note = os.path.join(repo, "notes.txt")
        with open(note, "w", encoding="utf-8") as fh:
            fh.write("not a build input\n")
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "build", "sources.sha256")):
            text = open(p, encoding="utf-8").read()
            h = hashlib.sha256(b"not a build input\n").hexdigest()
            with open(p, "w", encoding="utf-8") as fh:
                fh.write(text + f"{h}  notes.txt\n")
        return tmp
    expect("清单含枚举规则外条目 → FAIL", False, "枚举规则之外", manifest_extra_entry)

    def fake_repo_escape(tmp):
        """证据自带伪 repo(非 Cargo 工作区)且未设 RH_VERIFY_REPO:必须 FAIL。"""
        build_good_set(tmp)
        fake = os.path.join(tmp, "fake-repo")
        os.makedirs(fake, exist_ok=True)
        import glob
        for p in glob.glob(os.path.join(tmp, "*", "build", "binaries.json")):
            doc = json.load(open(p, encoding="utf-8"))
            doc["repo_root"] = fake
            with open(p, "w", encoding="utf-8") as fh:
                json.dump(doc, fh)
        return tmp
    expect("证据自带伪 repo 逃避绑定 → FAIL", False, "不像 Cargo 工作区", fake_repo_escape)

    def raw_target_mismatch(tmp):
        """记录声明 tunnel(dst=10.0.0.1:20002),raw connected 却是直连目标。"""
        build_good_set(tmp)
        d = os.path.join(tmp, "tcp-nomux")
        f = [x for x in os.listdir(d) if x.startswith("results-")][0]
        fp = os.path.join(d, f)
        recs = [json.loads(x) for x in open(fp, encoding="utf-8") if x.strip()]
        for r in recs:
            if r.get("metric") == "throughput" and r.get("path") == "tunnel" \
                    and r.get("streams") == 1 and r.get("dir") == "upload":
                raw_path = os.path.join(d, r["raw_json"])
                raw = json.loads(open(raw_path, encoding="utf-8").read())
                raw["start"]["connected"][0] = {"remote_host": "10.0.0.2",
                                                "remote_port": 19101}
                with open(raw_path, "w", encoding="utf-8") as fh:
                    json.dump(raw, fh)
                break
        return tmp
    expect("raw 测试目标与声明不符(隧道记录实跑直连)→ FAIL", False,
           "remote_port", raw_target_mismatch)

    all_ok = all(r[1] for r in results)
    print("== verify_goal --self-test ==")
    for name, ok, detail in results:
        print(f"[{'PASS' if ok else 'FAIL'}] {name}")
        if detail:
            print(f"       {detail}")
    print(f"自检 {'通过' if all_ok else '失败'}({sum(1 for r in results if r[1])}/{len(results)})")
    return all_ok


def main():
    import argparse
    ap = argparse.ArgumentParser(description="goal-repair 证据验收核验器")
    ap.add_argument("--evidence", help="证据目录(单个,或包含多个证据目录的父目录)")
    ap.add_argument("--self-test", action="store_true",
                    help="对抗性自检:验证核验器对缺证据/旧口径/伪造证据 fail closed")
    args = ap.parse_args()
    if args.self_test:
        return 0 if run_self_test() else 1
    if not args.evidence:
        ap.print_help()
        return 2
    ok, report = verify_path(args.evidence)
    print(report)
    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
