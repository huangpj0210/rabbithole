#!/usr/bin/env python3
"""g1_tcp —— G1 TCP nomux 单流专项验收装置(标准库;复用 closure_c84 机制)。

范围:仅 G1(verify_goal 的 G1 gate 分支),不宣称 G2-G7。复用 closure_c84 的
确定性源快照/三机 Termark 通道/PID 治理/双端 CPU/raw schema,不弱化任何既有
检查;本工具独占 benchmark/g1_tcp.py 与 benchmark/py/tests/test_g1_tcp.py。

子命令(全套与远端测量由工作流脚本统一执行;本工具不自跑):
  prepare  --out-dir ROOT   只创建不存在目录(ROOT 或仅含本工具静态契约文件);
                            保存当前 HEAD/status、所有既有修改的基线副本与 SHA;
                            当前树(未提交,不用 git archive HEAD)baseline-source
                            完整可构建源快照;创建 gates/;写工具/测试自身 SHA。
  measure  --out-dir DIR [--source TREE] [--diagnostics] [--final]
                            指定 TREE 时冻结 TREE 副本,否则冻结当前工作区;
                            Linux cargo test --workspace --locked + cargo build
                            --release --locked(真实 exit,失败留证据并非零);
                            test-workspace/build-release log/.exit 回收到 DIR/logs;
                            三机 TCP mux=0 relay=splice 每方向单流 10s × ≥3
                            direct/tunnel 配对(默认 2 个独立窗口轮;peer.sh 自带
                            多流/UDP/延迟/并发留作诊断,绝不混为单流);
                            --final 同构建完整执行并回收四套 E2E;验收轮无任何
                            实验环境开关、诊断计数默认关闭;
                            --diagnostics 额外独立诊断轮(证据置于 DIR/diagnostics,
                            不进证据根):只读带时间 ss -tinm 三段 TCP、真实 rh
                            PID/TID /proc 与管道容量(不采 wrapper PID)、strace
                            独立受扰动短测(不进入最终样本)。
  verify   --run-dir DIR --repo TREE --json [--require-e2e]
                            stdout 只输出单个严格 JSON(schema=g1-tcp-verdict-v1):
                            evidence_valid/g1_pass/regressions_pass(bool)、
                            upload_mbps/download_mbps(number|null)、
                            failures/evidence_paths(string[])。import 未改的
                            verify_goal.EvidenceDir/evaluate_gates,读返回的 G1
                            Gate 对象(不用 CLI 总退出码);固定 RH_VERIFY_REPO=TREE;
                            额外检查:invalid/失败不可静默过滤、真实 Linux
                            test/build log+exit、source/bin/endpoint 与 declared
                            tree 绑定、raw 单流方向/目标、≥3 独立配对、CPU
                            覆盖/单核、nofile、正常 splice 路径;--require-e2e
                            四套原件实际 PASS;有效证据但吞吐低 = FAIL(非环境
                            无效);fresh direct 下行中位 <3000(≥3对、≥2 独立
                            窗口)时给出'环境受限 FAIL'判定,禁止无限候选迭代。
  fixed-ab --out-dir DIR [--source TREE]
                            候选树有 crates/rh-transport/examples/g1_splice_bench.rs
                            时:基线(prepare 的 baseline-source + 候选 example
                            原样叠加,同计数同钩子同 release flags)与候选在
                            Linux 交替 ≥3 组、每臂双向固定工作量 ≥128MiB,保留
                            每步计数与小包 p50/p99;没有 example 则明确非零
                            '不可用',绝不冒充三机达标。example CLI 契约见
                            benchmark/runs/g1-tcp-20261014/example-contract.md。
  --self-test               本地对抗自检(不碰远端)。

红线(与工作流一致):不动 sysctl/NIC/irq;不碰 SO_RCVBUF/SO_SNDBUF;无新协议;
计量/配额/取消/半关闭语义由既有测试锁定,本工具不触产品代码;远端只清理本工具
记录且身份核验通过的进程;凭据只落 0600 文件且报告脱敏;新证据只增不覆盖;
runner(drive)退出码不用于判定性能达标。
"""

from __future__ import annotations

import argparse
import datetime as _dt
import hashlib
import json
import os
import re
import shlex
import shutil
import sys
import tarfile
import tempfile
import time

BENCH_DIR = os.path.dirname(os.path.abspath(__file__))
PY_DIR = os.path.join(BENCH_DIR, "py")
REPO_ROOT = os.path.dirname(BENCH_DIR)
sys.path.insert(0, BENCH_DIR)
sys.path.insert(0, PY_DIR)

import closure_c84 as cc  # noqa: E402  复用既有编排机制(不改 closure_c84)
import verify_goal as vg  # noqa: E402  未改的核验器(G1 gate 判定的单一事实来源)

# ---------------------------------------------------------------- 常量

SCHEMA = "g1-tcp-run"
SCHEMA_VERSION = 1
VERDICT_SCHEMA = "g1-tcp-verdict-v1"

DEFAULT_ROOT = os.path.join(REPO_ROOT, "benchmark", "runs", "g1-tcp-20261014")

# G1 专项固定配置:TCP、mux=0、relay=splice、单流、10s
G1_PROTO = "tcp"
G1_MUX = 0
G1_RELAY = "splice"
G1_STREAMS = 1
G1_CONC_LEVELS = "64"           # 仅 nofile 门控用(64+512 << 65535),并发档为诊断附带

IPERF3_PATH = "/tmp/rh-bin/iperf3"
IPERF3_WANT = "3.17.1"

# 环境上限退出准则(独立复核修正1):fresh direct 下行中位仍 <3000 时,
# G1-下行按当前三机环境判'环境受限 FAIL',停止候选迭代,保留 default。
ENV_LIMIT_DIRECT_MBPS = vg.THRESH["g1_mbps"]     # 3000
ENV_LIMIT_MIN_PAIRS = vg.THRESH["g7_min_pairs"]  # 3
ENV_LIMIT_MIN_WINDOWS = 2

# fixed-ab:候选 example 的固定位置与工作量
EXAMPLE_REL = os.path.join("crates", "rh-transport", "examples", "g1_splice_bench.rs")
FIXED_AB_MIB = 128
FIXED_AB_ROUNDS = 3

# 工具与测试自身 SHA(不藏版本)
TESTS_REL = os.path.join("benchmark", "py", "tests", "test_g1_tcp.py")
STATIC_PREPARE_FILES = ("example-contract.md",)   # 工具自有静态文件(允许已存在)

HEX64 = re.compile(r"^[0-9a-f]{64}$")

# example-contract.md 的规范内容(工具在 prepare 时落盘;已存在则必须逐字节一致,
# 绝不覆盖)。
EXAMPLE_CONTRACT = """\
# g1_splice_bench —— G1 固定工作量对照 example 契约(v1,验收装置工程师定义)

TCP 实现工程师按本契约在候选树新增 `crates/rh-transport/examples/g1_splice_bench.rs`,
由 `benchmark/g1_tcp.py fixed-ab` 在 Linux 上做基线/候选交替对照。契约即验收:
不满足任意一条,fixed-ab 拒绝运行(非零,不可用),不得口头解释替代。

## 1. 硬性约束

1. 位置与名字固定:`crates/rh-transport/examples/g1_splice_bench.rs`;不新增
   依赖、不修改任何 `Cargo.toml`(fixed-ab 会把该文件**原样叠加**到 prepare
   阶段的 `baseline-source` 快照上构建旧泵对照臂,因此它必须只依赖工作区
   既有 crates 与 std,叠加后可编译)。
2. 必须真实驱动**生产泵**(rh-transport 中 data.rs 实际装配的 splice 泵,
   经由其公开类型/函数;禁止 example 内重新实现一份拷贝泵)。
3. 基线臂与候选臂:同一 example 文件、同一计数器集合、同一钩子、同一
   `cargo build --release --locked` 默认 flags(fixed-ab 构建时 unset RUSTFLAGS,
   不加任何实验开关);唯一差异是底层泵实现。
4. 不修改任何系统配置、不动 SO_RCVBUF/SO_SNDBUF;本地 socketpair/pipe 即可
   (它只做归因对照,不冒充三机 G1 达标)。

## 2. CLI(fixed-ab 按此调用,参数名/语义不得偏差)

```
g1_splice_bench --direction upload|download --mib <N> [--json]
```

* `--direction upload`:本地源 → 泵 → 本地汇聚(对应上行 fill→drain 方向);
  `--direction download`:反向。两方向都被调用并各自留 JSON。
* `--mib N`:固定工作量,N≥128(fixed-ab 默认 128)。
* `--json`:stdout 输出**单个**严格 JSON(拒绝 NaN/Infinity);其它内容只进 stderr。
* 退出码:0=完成;非 0=失败(失败时也输出含 `error` 字段的 JSON)。

## 3. stdout JSON schema(fixed-ab-v1)

```json
{
  "schema": "fixed-ab-v1",
  "direction": "upload",
  "mib": 128,
  "bytes": 134217728,
  "elapsed_s": 0.0,
  "gbps": 0.0,
  "cpu": {"process_pct_single_core": 0.0, "user_s": 0.0, "sys_s": 0.0},
  "counters": {
    "fill_calls": 0, "fill_bytes": 0,
    "drain_calls": 0, "drain_bytes": 0,
    "eagain_fill_input_empty": 0,
    "eagain_drain_pipe_full": 0,
    "eagain_drain_write_full": 0,
    "pending_waits_read": 0,
    "pending_waits_write": 0,
    "coop_yields": 0
  },
  "per_mib": [
    {"mi": 1, "t_s": 0.0, "fill": 0, "drain": 0, "eagain": 0, "pending": 0}
  ],
  "small_packet": {"count": 0, "size": 0, "p50_us": 0.0, "p99_us": 0.0}
}
```

* 计数器由泵本地累计、结束输出;不在热路径加全局锁/原子热点。
* `eagain_*` 按实际来源方向分开计(读侧空/管道满/写侧满)——对照
  EAGAIN 归因边界:管道满必须转 drain,不得当读侧 EAGAIN 清读侧就绪。
* `pending_waits_*` 只计**真实 Pending**(readiness 等待),ready 立即返回不计。
* `per_mib`:每满 1MiB 一条(≥mib 条)。
* `small_packet`:固定 8192 次 × 64B 请求/响应小包往返(低负载),报告 p50/p99
  微秒;SPLICE_F_MORE/NOTSENT_LOWAT 等节律改动若影响小包时延,须在此可见。
* `elapsed_s`/`gbps` 用单调钟;CPU 用 /proc 自身 utime+stime 差分(单核口径)。

## 4. 公平性/抗饿死观察(独立复核修正2)

example 需能在单 worker 语义下暴露泵独占问题(若提供多 relay 并发模式,计数器
中显式报告;实现工程师保证候选泵每批次有字节/操作预算+显式让步,`coop_yields`
即预算触发点)。fixed-ab 只归因,不设时延/吞吐门槛替代三机 G1 判定。
"""


def now_iso() -> str:
    return _dt.datetime.now(_dt.timezone.utc).isoformat()


def die(msg: str) -> None:
    print(f"[g1][fail] {msg}", file=sys.stderr)
    raise SystemExit(1)


def sha256_file(path: str) -> str:
    return cc.sha256_file(path)


def tool_shas() -> dict:
    """工具与测试自身 SHA 单独记录(不藏版本)。"""
    return {
        "g1_tcp_sha256": sha256_file(os.path.abspath(__file__)),
        "test_g1_tcp_sha256": sha256_file(os.path.join(REPO_ROOT, TESTS_REL)),
        "closure_c84_sha256": sha256_file(os.path.join(BENCH_DIR, "closure_c84.py")),
        "verify_goal_sha256": sha256_file(os.path.join(PY_DIR, "verify_goal.py")),
    }


# ---------------------------------------------------------------- run-index

def index_load(out_dir: str) -> dict:
    p = os.path.join(out_dir, "run-index.json")
    if os.path.isfile(p):
        return json.load(open(p, encoding="utf-8"))
    return {"schema": SCHEMA, "version": SCHEMA_VERSION, "rounds": [],
            "diagnostics_rounds": [], "e2e": {}, "created_at": now_iso()}


def index_save(out_dir: str, idx: dict) -> None:
    idx["updated_at"] = now_iso()
    with open(os.path.join(out_dir, "run-index.json"), "w", encoding="utf-8") as fh:
        json.dump(idx, fh, ensure_ascii=False, indent=2, sort_keys=True)


def _write_new_file(path: str, text: str, mode: int = 0o644) -> None:
    """只增不覆盖:目标已存在即中止(证据/基线永不覆盖)。"""
    if os.path.exists(path):
        die(f"拒绝覆盖既有文件(新证据只增): {path}")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, mode)
    with os.fdopen(fd, "w", encoding="utf-8") as fh:
        fh.write(text)


# ================================================================ prepare

def _git(repo: str, *args) -> str:
    rc, out, err = cc.run_local(["git", "-C", repo, *args], deadline_s=120)
    if rc != 0:
        die(f"git {' '.join(args)} 失败: {err.decode('utf-8', 'replace')[-300:]}")
    return out.decode("utf-8", "replace")


def cmd_prepare(args) -> int:
    root = os.path.abspath(args.out_dir)
    if os.path.exists(root) and not os.path.isdir(root):
        die(f"--out-dir 是文件,不是目录: {root}")
    if os.path.isdir(root):
        extra = sorted(set(os.listdir(root)) - set(STATIC_PREPARE_FILES))
        if extra:
            die(f"--out-dir 已存在且含非本工具静态文件(基线/历史证据永不覆盖): "
                f"{root} 多出 {extra[:8]}")
    else:
        os.makedirs(root)
    # 静态契约文件:缺失则写;已存在必须逐字节一致(不覆盖)
    contract_path = os.path.join(root, "example-contract.md")
    if os.path.exists(contract_path):
        cur = open(contract_path, encoding="utf-8").read()
        if cur != EXAMPLE_CONTRACT:
            die(f"{contract_path} 内容与工具内嵌契约不一致(不覆盖,请人工核对)")
    else:
        _write_new_file(contract_path, EXAMPLE_CONTRACT)

    head = _git(REPO_ROOT, "rev-parse", "HEAD").strip()
    status = _git(REPO_ROOT, "status", "--porcelain", "-uall")
    _write_new_file(os.path.join(root, "HEAD.txt"), head + "\n")
    _write_new_file(os.path.join(root, "git-status.txt"), status)
    _write_new_file(os.path.join(root, "git-status.sha256"),
                    hashlib.sha256(status.encode()).hexdigest() + "\n")

    # 所有既有修改的基线副本 + SHA(M=已修改/??=未跟踪/D=已删除,如实记录)
    mods_dir = os.path.join(root, "baseline-modifications")
    entries = {}
    for line in status.splitlines():
        if not line.strip():
            continue
        st, path = line[:2], line[3:]
        rec = {"status": st, "path": path}
        src = os.path.join(REPO_ROOT, path)
        if os.path.isfile(src) and not os.path.islink(src):
            dst = os.path.join(mods_dir, path)
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.copy2(src, dst)
            rec["sha256"] = sha256_file(src)
            rec["baseline_copy"] = os.path.join("baseline-modifications", path)
        elif os.path.islink(src):
            rec["symlink"] = True
        else:
            rec["missing_or_deleted"] = True
        entries[path] = rec
    _write_new_file(os.path.join(root, "baseline-modifications.json"),
                    json.dumps({"created_at": now_iso(), "entries": entries},
                               ensure_ascii=False, indent=2, sort_keys=True))

    # 当前树(未提交,不用 git archive HEAD)baseline-source 完整可构建源快照
    snap = os.path.join(root, "baseline-source")
    if os.path.exists(snap):
        die(f"拒绝覆盖既有 baseline-source: {snap}")
    meta = cc.snapshot_workspace(REPO_ROOT, snap)
    doc = cc.write_build_inputs(snap, os.path.join(root, "baseline-build"),
                                {"snapshot_rule": meta["rule"],
                                 "git_head": meta.get("git_head"),
                                 "purpose": "fixed-ab 基线臂 + G1 起点绑定"})
    gates = os.path.join(root, "gates")
    os.makedirs(gates, exist_ok=True)
    if os.listdir(gates):
        die(f"gates/ 已有内容(不覆盖): {gates}")
    _write_new_file(os.path.join(root, "prepared.json"),
                    json.dumps({"created_at": now_iso(),
                                "git_head": head,
                                "git_status_sha256": hashlib.sha256(
                                    status.encode()).hexdigest(),
                                "baseline_source": {
                                    "rule": meta["rule"],
                                    "file_count": meta["file_count"],
                                    "tree_hash": doc["tree_hash"]},
                                "example_contract_sha256": sha256_file(contract_path),
                                **tool_shas()},
                               ensure_ascii=False, indent=2, sort_keys=True))
    print(f"[g1] prepare 完成: {root}(baseline tree_hash={doc['tree_hash'][:16]}…, "
          f"修改基线 {len(entries)} 项)")
    return 0


# ================================================================ measure

def check_iperf_reality(tm: cc.Tm) -> dict:
    """工具实况:三机 iperf 必须 /tmp/rh-bin/iperf3 3.17.1(勿用系统 3.9);
    ss/strace 有无如实记录(无 perf,不能安装)。"""
    info = {}
    for name, asset in cc.ASSETS.items():
        out = tm.exec_sh(asset["id"],
                         f"{shlex.quote(IPERF3_PATH)} --version 2>&1 | head -1; "
                         f"command -v ss >/dev/null && echo ss=ok || echo ss=MISSING; "
                         f"command -v strace >/dev/null && echo strace=ok "
                         f"|| echo strace=MISSING; "
                         f"command -v perf >/dev/null && echo perf=present || echo perf=absent",
                         name=f"iperf3-reality-{name}", check=False)
        ver_line = out.strip().splitlines()[0] if out.strip() else ""
        info[name] = {"iperf3": ver_line, "raw": out.strip()}
        if IPERF3_WANT not in ver_line:
            die(f"工具实况不符({name}):{IPERF3_PATH} 版本必须是 {IPERF3_WANT},"
                f"实测 {ver_line!r}(不安装/不换路径,中止)")
    return info


def _freeze_source(progress: cc.Progress, source: str | None) -> tuple:
    """冻结源:指定 TREE 用其确定性副本,否则静止校验后冻结当前工作区。
    返回 (snap_dir, meta)。"""
    tree = os.path.abspath(source) if source else REPO_ROOT
    if not os.path.isdir(tree):
        die(f"--source 不是目录: {tree}")
    cc._quiesce_check(tree, progress)   # 冻结前后内容哈希一致(防并行改动)
    snap = os.path.join(tempfile.mkdtemp(prefix="rh-g1-freeze-"), "src")
    meta = cc.snapshot_workspace(tree, snap)
    progress.event("源已冻结" + ("(指定 TREE)" if source else "(当前工作区)"),
                   f"{tree} → {snap} ({meta['file_count']} 文件)")
    return snap, meta


def sync_source_g1(tm: cc.Tm, progress: cc.Progress, out_dir: str, snap: str,
                   mode: str, stamp: str) -> str:
    """打包快照(确定性集 + 当前工具叠加:benchmark/g1_tcp.py 与
    benchmark/closure_c84.py 以当前工具为准,同 closure 的 harness overlay
    语义;工具不参与 build-inputs)→ 三机独立目录;远端重算 build-inputs 与
    本地逐项匹配;两个工具文件哈希逐一核对。"""
    remote_root = f"/tmp/rh-g1-tcp-{mode}-{stamp}"
    tar_path = os.path.join(out_dir, f"source-{mode}-{stamp}.tar")
    with tarfile.open(tar_path, "w") as tf:
        for rel in cc.enumerate_workspace_files(snap):
            tf.add(os.path.join(snap, rel), arcname=rel, recursive=False)
        tf.add(os.path.abspath(__file__), arcname="benchmark/g1_tcp.py",
               recursive=False)
        tf.add(os.path.join(BENCH_DIR, "closure_c84.py"),
               arcname="benchmark/closure_c84.py", recursive=False)
    tar_sha = sha256_file(tar_path)
    meta = json.load(open(os.path.join(out_dir, "build", "build-inputs.json"),
                          encoding="utf-8"))
    tool_want = {"g1_tcp.py": sha256_file(os.path.abspath(__file__)),
                 "closure_c84.py": sha256_file(os.path.join(BENCH_DIR,
                                                            "closure_c84.py"))}
    for name in ("test1", "test2", "test3"):
        asset = cc.ASSETS[name]
        tm.exec_sh(asset["id"], f"mkdir -p {remote_root} && chmod 700 {remote_root}",
                   name=f"mkdir-{name}")
        tm.upload(asset["id"], tar_path, f"{remote_root}/src.tar", name=f"ul-{name}")
        got = tm.exec_sh(asset["id"],
                         f"sha256sum {remote_root}/src.tar | awk '{{print $1}}'",
                         name=f"sha-{name}").strip()
        if got != tar_sha:
            die(f"{name} 源包哈希不一致: {got} != {tar_sha}")
        tm.exec_sh(asset["id"],
                   f"cd {remote_root} && rm -rf tree && mkdir tree && "
                   f"tar xf src.tar -C tree && "
                   f"python3 tree/benchmark/closure_c84.py _internal-build-inputs-check "
                   f"--tree tree --expect {shlex.quote(meta['tree_hash'])}",
                   name=f"verify-inputs-{name}")
        for tool, want in tool_want.items():
            got = tm.exec_sh(asset["id"],
                             f"sha256sum {remote_root}/tree/benchmark/{tool} "
                             f"| awk '{{print $1}}'",
                             name=f"toolsha-{tool}-{name}").strip()
            if got != want:
                die(f"{name} 远端工具 {tool} 哈希不一致: {got} != {want}"
                    f"(叠加/传输损坏,拒绝构建)")
        progress.event("源已同步并逐项核对", f"{name} tree_hash+工具哈希一致")
    return remote_root


def _collect_build_logs(tm: cc.Tm, out_dir: str, remote_root: str) -> None:
    """targeted 不回收构建日志;本工具必须:真实 log/.exit 落 DIR/logs。"""
    lg = os.path.join(out_dir, "logs")
    os.makedirs(lg, exist_ok=True)
    for f in ("test-workspace.log", "test-workspace.exit",
              "build-release.log", "build-release.exit"):
        try:
            tm.download(cc.ASSETS["test1"]["id"], f"{remote_root}/logs/{f}",
                        os.path.join(lg, f), name=f"dl-log-{f}")
        except (cc.TmError, cc.CommandTimeout) as exc:
            print(f"[g1][warn] 构建日志回收失败({f}): {exc}", file=sys.stderr)


def _guarded_remote_build(tm: cc.Tm, out_dir: str, remote_root: str, toolchain: str,
                          progress: cc.Progress) -> dict:
    """remote_build 真实失败也要先把日志回收本地(失败留证据),再以非零退出。"""
    try:
        return cc.remote_build(tm, out_dir, remote_root, toolchain)
    except SystemExit:
        progress.event("构建/测试失败,回收失败证据后中止", remote_root)
        _collect_build_logs(tm, out_dir, remote_root)
        raise


class G1RoundRunner(cc.RoundRunner):
    """closure RoundRunner 扩展:证据根可重定向(诊断臂置于证据根之外)。

    ★事故回归(2026-10-15 baseline 首跑):实例属性不得叫 `diagnostics` ——
    closure RoundRunner.run() 会调用 self.diagnostics()(方法),bool 属性
    遮蔽方法 → 'bool' object is not callable → collect() 不执行、证据丢失。
    诊断标记一律用 diag_mode(单测锁定 diagnostics 保持可调用)。"""

    def __init__(self, tm, progress, out_dir, remote_root, cfg,
                 evidence_root: str = None, diag_mode: bool = False):
        super().__init__(tm, progress, out_dir, remote_root, cfg)
        self.evidence_root = evidence_root or os.path.join(out_dir, "evidence")
        self.diag_mode = diag_mode

    # ---- 证据回收(与 closure 同 schema,证据根重定向)--------------------
    def collect(self) -> str:
        ev_dir = os.path.join(self.evidence_root, self.cfg.label)
        os.makedirs(ev_dir, exist_ok=True)
        tmp = tempfile.mkdtemp(prefix="rh-g1-collect-")
        try:
            self.tm.download(cc.ASSETS["test3"]["id"], f"{self.rdir}/drive",
                             os.path.join(tmp, "drive"), name="dl-drive")
            self.tm.download(cc.ASSETS["test1"]["id"], f"{self.rdir}/server",
                             os.path.join(tmp, "server"), name="dl-server")
            self.tm.download(cc.ASSETS["test2"]["id"], f"{self.rdir}/client",
                             os.path.join(tmp, "client"), name="dl-client")
            for name in os.listdir(os.path.join(tmp, "drive")):
                if (name.startswith("results-") and name.endswith(".jsonl")) \
                        or name in ("schema.json",):
                    shutil.copy2(os.path.join(tmp, "drive", name),
                                 os.path.join(ev_dir, name))
            if os.path.isdir(os.path.join(tmp, "drive", "raw")):
                shutil.copytree(os.path.join(tmp, "drive", "raw"),
                                os.path.join(ev_dir, "raw"))
            for role in ("server", "client"):
                dst = os.path.join(ev_dir, "endpoints", role)
                os.makedirs(dst, exist_ok=True)
                for f in ("endpoint.json", "identity.jsonl", "cpu.jsonl",
                          "nofile.json"):
                    src = os.path.join(tmp, role, f)
                    if os.path.isfile(src):
                        shutil.copy2(src, os.path.join(dst, f))
                src_log = os.path.join(tmp, role, f"{role}.log")
                if os.path.isfile(src_log):
                    dlog = os.path.join(ev_dir, "endpoint-logs")
                    os.makedirs(dlog, exist_ok=True)
                    shutil.copy2(src_log, os.path.join(dlog, f"{role}.log"))
                    txt = open(src_log, encoding="utf-8", errors="replace").read()
                    with open(os.path.join(dlog, f"{role}.log.redacted"), "w",
                              encoding="utf-8") as fh:
                        fh.write(cc.redact(txt))
            build_src = os.path.join(self.out_dir, "build")
            if os.path.isdir(build_src):
                dst = os.path.join(ev_dir, "build")
                os.makedirs(dst, exist_ok=True)
                for f in ("sources.sha256", "binaries.json", "build-inputs.sha256",
                          "build-inputs.json"):
                    src = os.path.join(build_src, f)
                    if os.path.isfile(src):
                        shutil.copy2(src, os.path.join(dst, f))
                bdir = os.path.join(dst, "bin")
                os.makedirs(bdir, exist_ok=True)
                for b in ("rh-server", "rh-client"):
                    srcb = os.path.join(build_src, "bin", b)
                    if os.path.isfile(srcb):
                        shutil.copy2(srcb, os.path.join(bdir, b))
        finally:
            shutil.rmtree(tmp, ignore_errors=True)   # 凭据只在 out_dir/.secrets(0600)
        for name in os.listdir(ev_dir):
            if name.startswith("results-"):
                p = os.path.join(ev_dir, name)
                txt = open(p, encoding="utf-8").read()
                with open(p + ".redacted", "w", encoding="utf-8") as fh:
                    fh.write(cc.redact(txt))
        self.progress.event("证据已回收组装", self.cfg.label, evidence=ev_dir)
        return ev_dir

    # ---- 诊断轮:drive 期间采样(只读;不采 wrapper PID)------------------
    def _rh_pid_of(self, sub: str, pname: str):
        """从 peer.sh 自己写的 pid 文件取**真实服务 PID**(server.pid/client.pid
        = rh 进程本体;server-role.pid/client-role.pid 是 setsid wrapper,禁采)。"""
        for _asset, tag, pid, pn in self.roles:
            if tag == f"{sub}/{pname}":
                return pid
        return None

    def _diag_tick_cmd(self, sub: str, host_pnames: tuple) -> str:
        parts = [f"echo '===== tick $(date -Is) {sub} ====='"]
        parts.append("ss -tinm 2>/dev/null")          # 三段 TCP 队列/cwnd/notsent 等
        for pname in host_pnames:
            pid = self._rh_pid_of(sub, pname)
            if pid is None:
                continue
            parts.append(
                f"p={pid}; if [ -d /proc/$p ]; then "
                f"echo '--- proc $p ({pname}) exe='$(readlink /proc/$p/exe)"
                f"' threads='$(ls /proc/$p/task 2>/dev/null | wc -l); "
                f"grep -E 'voluntary_ctxt_switches|nonvoluntary_ctxt_switches' "
                f"/proc/$p/status 2>/dev/null; "
                f"for t in $(ls /proc/$p/task 2>/dev/null); do "
                f"awk -v t=$t '{{print \"tid=\"t\" utime=\"$14\" stime=\"$15}}' "
                f"/proc/$p/task/$t/stat 2>/dev/null; done; "
                f"echo '--- pipe_fds:'$(ls -l /proc/$p/fd 2>/dev/null "
                f"| grep -c 'pipe:'); fi")
        parts.append("python3 -c 'import os,fcntl;r,w=os.pipe();"
                     "print(\"pipe_sz_default=\"+str(fcntl.fcntl(r,103)))' 2>/dev/null "
                     "|| echo pipe_sz_default=unavailable; "
                     "echo pipe_max=$(cat /proc/sys/fs/pipe-max-size 2>/dev/null)")
        return "; ".join(parts)

    def _tick_append_cmd(self, sub: str, host_pnames: tuple, log_path: str) -> str:
        """★事故回归(round-4 诊断轮):分号列表上直接接 `>> log 2>&1` 只会重定向
        最后一条简单命令(实测仅落了 pipe_max 一行/tick,其余输出进 termark
        通道日志)。整个 tick 必须包进子 shell 再整体重定向。"""
        return f"( {self._diag_tick_cmd(sub, host_pnames)} ) >> {log_path} 2>&1"

    def run_drive_with_diag(self, max_ticks: int = 60, poll_s: float = 15.0) -> int:
        """非阻塞拉起 drive → 周期采样 ss/proc/pipe(带时间戳)→ 轮询退出码。"""
        a1, a3 = cc.ASSETS["test1"], cc.ASSETS["test3"]
        sec_dir = os.path.join(self.out_dir, ".secrets")
        token_local = os.path.join(sec_dir, f"{self.cfg.label}-token")
        self.tm.download(a1["id"], f"{self.rdir}/server/token", token_local,
                         name="dl-token")
        os.chmod(token_local, 0o600)
        d = f"{self.rdir}/drive"
        self.tm.exec_sh(a3["id"], f"mkdir -p {d}", name="mkdir-drive")
        self.tm.upload(a3["id"], token_local, f"{d}/token", name="ul-token")
        cid_remote = f"{d}/cid"
        cid_local = os.path.join(sec_dir, f"{self.cfg.label}-cid")
        self.tm.upload(a3["id"], cid_local, cid_remote, name="ul-cid")
        self.tm.exec_sh(a3["id"], f"chmod 600 {d}/token {cid_remote}",
                        name="chmod-drive-creds")
        ev = cc.peer_env(self.cfg, self.remote_root)
        ev["PEER_DIR"] = d
        cmd = (f"cd {self.src_root}/benchmark && {cc._env_prefix(ev)} "
               f"bash peer.sh drive {cc.ASSETS['test1']['ip']} {cc.ASSETS['test2']['ip']} "
               f"{cc.PORT_PLAN['api']} \"$(cat {d}/token)\" \"$(cat {cid_remote})\" "
               f"{self.cfg.label}")
        self.progress.event("诊断压测轮启动(周期采样)",
                            f"{self.cfg.label}(受扰动样本不进入验收证据)")
        pid, ident = cc.remote_launch(self.tm, a3, cmd, d, "drive")
        exit_f = f"{d}/drive.exit"

        def _state():
            rc, out = self.tm.exec(a3["id"],
                                   f"if [ -f {exit_f} ]; then cat {exit_f}; "
                                   f"elif kill -0 {pid} 2>/dev/null; then echo RUNNING; "
                                   f"else echo LOST; fi",
                                   deadline=cc.DEADLINES["drive_poll"],
                                   name="poll-diag-drive", check=False)
            return out.strip().splitlines()[-1] if out.strip() else "LOST"

        tick = 0
        deadline_ts = time.time() + self.cfg.drive_deadline
        state = _state()
        while state == "RUNNING" and time.time() < deadline_ts:
            if tick < max_ticks:
                for sub, asset, host_pnames in (
                        ("server", a1, ("server.pid",)),
                        ("client", cc.ASSETS["test2"],
                         ("client.pid", "iperf.pid", "target.pid"))):
                    self.tm.exec(asset["id"],
                                 self._tick_append_cmd(
                                     sub, host_pnames,
                                     f"{self.rdir}/{sub}/diag-ss-proc.log"),
                                 deadline=cc.DEADLINES["exec_small"],
                                 name=f"diag-tick-{sub}-{tick}", check=False)
                tick += 1
            time.sleep(poll_s)
            state = _state()
        if state == "RUNNING":
            cc.governed_kill(self.tm, a3, pid, ident, "drive")
            raise cc.CommandTimeout(f"诊断 drive 超时已治理清理: {self.cfg.label}")
        if state == "LOST":
            raise cc.TmError(f"{self.cfg.label} drive 进程丢失且无退出码文件")
        try:
            return int(state)
        except ValueError:
            die(f"诊断 drive 退出码不可解析: {state!r}")

    def strace_probe(self) -> dict:
        """strace 独立受扰动短测:attach 本轮记录的 server PID,3s 单流探针;
        ptrace 受限时如实记录;结果绝不进入验收样本。"""
        a1 = cc.ASSETS["test1"]
        pid = self._rh_pid_of("server", "server.pid")
        if pid is None:
            return {"error": "未找到本轮记录的 server.pid(不 attach 未知进程)"}
        probe = (f"cd {self.rdir}/server && "
                 f"(timeout -s INT 15 strace -f -c -p {pid} "
                 f"-o strace-server.summary 2>strace-server.err; "
                 f"echo $? > strace-server.exit) & "
                 f"sleep 1; {shlex.quote(IPERF3_PATH)} -c {cc.ASSETS['test1']['ip']} "
                 f"-p {cc.PORT_PLAN['proxy_base'] + 1} -t 3 -P {G1_STREAMS} -J "
                 f"> strace-probe.json 2> strace-probe.stderr; "
                 f"echo $? > strace-probe.exit; sleep 13; "
                 f"echo probe_done")
        out = self.tm.exec_sh(a1["id"], probe, deadline=60,
                              name="strace-probe", check=False)
        return {"server_pid": pid, "raw": out.strip()[-2000:],
                "note": "受 strace 扰动,仅诊断归因;不进入验收样本"}

    def collect_diag(self) -> dict:
        """诊断产物回收(置于证据根之外的 diagnostics/<label>/)。"""
        ddir = os.path.join(self.out_dir, "diagnostics", self.cfg.label)
        os.makedirs(ddir, exist_ok=True)
        for asset, sub in ((cc.ASSETS["test1"], "server"),
                           (cc.ASSETS["test2"], "client")):
            files = ["diag-ss-proc.log"]
            if sub == "server":      # strace 只 attach server 侧
                files += ["strace-server.summary", "strace-server.err",
                          "strace-server.exit", "strace-probe.json",
                          "strace-probe.stderr", "strace-probe.exit"]
            for f in files:
                remote = f"{self.rdir}/{sub}/{f}"
                try:
                    self.tm.download(asset["id"], remote,
                                     os.path.join(ddir, f"{sub}-{f}"),
                                     name=f"dl-diag-{sub}-{f}")
                except (cc.TmError, cc.CommandTimeout) as exc:
                    print(f"[g1][warn] 诊断文件回收失败({sub}/{f}): {exc}",
                          file=sys.stderr)
        return {"dir": ddir}


def _measure_common(args, mode: str):
    out_dir = os.path.abspath(args.out_dir)
    if os.path.exists(out_dir) and os.listdir(out_dir):
        die(f"--out-dir 非空目录(新证据只增不覆盖): {out_dir}")
    os.makedirs(out_dir, exist_ok=True)
    progress = cc.Progress(out_dir)
    tm = cc.Tm(progress, os.path.join(out_dir, "logs"))
    stamp = _dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    idx = index_load(out_dir)
    idx.update({"mode": mode, "started_at": now_iso(),
                "source": os.path.abspath(args.source) if args.source else "workspace",
                "diagnostics": bool(args.diagnostics), **tool_shas()})
    return out_dir, progress, tm, stamp, idx


def cmd_measure(args) -> int:
    mode = "final" if args.final else "measure"
    out_dir, progress, tm, stamp, idx = _measure_common(args, mode)
    repeat = int(args.repeat)
    duration = int(args.duration)
    windows = int(args.windows)
    if repeat < vg.THRESH["g7_min_pairs"] or windows < 1:
        die(f"--repeat ≥{vg.THRESH['g7_min_pairs']}(G7) 且 --windows ≥1: "
            f"repeat={repeat} windows={windows}")
    if args.final and args.diagnostics:
        die("--final 验收轮与 --diagnostics 诊断轮必须分离运行(诊断采样扰动隔离)")
    # ★独立复核 blocker 1:measure 是否执行四套 E2E 必须在 run-index 显式可辨,
    # 防止非 final 轮的 regressions_pass 被误引为 E2E 证据。
    idx["e2e_policy"] = (
        {"final": True, "suites": [k for k, _ in cc.E2E_SUITES],
         "note": "--final:同构建完整执行并回收四套 E2E 原件"}
        if args.final else
        {"final": False, "suites": [],
         "note": "本次 measure 未带 --final:未执行四套 E2E、无 e2e-* 原件;"
                 "regressions_pass(verify)仅覆盖 cargo test+build。最终安全须"
                 "以 --final 同构建执行并以 verify --require-e2e 复判"})
    idx.update({"env_limit_watch": {
        "rule": "fresh direct 下行中位 <3000(≥3对、≥2独立窗口)→ G1-下行判"
                "环境受限 FAIL,停止候选迭代",
        "direct_threshold_mbps": ENV_LIMIT_DIRECT_MBPS,
        "min_pairs": ENV_LIMIT_MIN_PAIRS, "min_windows": ENV_LIMIT_MIN_WINDOWS}})
    index_save(out_dir, idx)
    # 工具实况 + 环境(时钟偏差/内核/nofile 只读观察)
    idx["iperf3_reality"] = check_iperf_reality(tm)
    for name, asset in cc.ASSETS.items():
        idx.setdefault("env", {})[name] = cc.check_remote_env(tm, asset)
    index_save(out_dir, idx)
    # 源冻结 + 构建输入清单
    snap, meta = _freeze_source(progress, args.source)
    toolchain = cc.remote_toolchain_pinned(snap) or "stable"
    doc = cc.write_build_inputs(snap, os.path.join(out_dir, "build"),
                                {"snapshot_rule": meta["rule"],
                                 "git_head": meta.get("git_head"),
                                 "toolchain_channel": toolchain})
    idx.update({"snapshot_rule": meta["rule"], "git_head": meta.get("git_head"),
                "tree_hash": doc["tree_hash"], "embedded_ui": doc["embedded_ui"],
                "toolchain_channel": toolchain})
    index_save(out_dir, idx)
    remote_root = sync_source_g1(tm, progress, out_dir, snap, mode, stamp)
    idx["remote_root"] = remote_root
    idx["source_snapshot"] = {
        "tar": f"source-{mode}-{stamp}.tar",
        "tar_sha256": sha256_file(os.path.join(out_dir, f"source-{mode}-{stamp}.tar")),
        "snapshot_rule": meta["rule"], "file_count": meta["file_count"]}
    index_save(out_dir, idx)
    build = _guarded_remote_build(tm, out_dir, remote_root, toolchain, progress)
    idx["build"] = build
    index_save(out_dir, idx)
    cc.pull_build_artifacts(tm, progress, out_dir, remote_root)
    _collect_build_logs(tm, out_dir, remote_root)
    # --final:同构建完整执行四套 E2E(真实 FAIL 非零即停,原件回收 DIR/logs)
    if args.final:
        progress.event("final:开始四套 E2E(e2e/resilience/transports/relay-modes)")
        cc.run_e2e_all(tm, progress, out_dir, remote_root, idx)
    # 验收轮:TCP mux=0 relay=splice 单流 10s × ≥3 direct/tunnel 配对/方向,
    # windows 轮 = windows 个独立窗口(独立 server/client/drive 生命周期)。
    # 无任何环境开关(诊断计数默认关闭;可疑实验开关一律不设)。
    for w in range(1, windows + 1):
        label = f"g1-tcp-nomux-w{w}"
        run_id = f"g1-{mode}-tcp-mux0-w{w}-{int(time.time())}"
        cfg = cc.RoundCfg(G1_PROTO, G1_MUX, label, run_id, repeat=repeat,
                          duration=duration, conc_levels=G1_CONC_LEVELS)
        runner = G1RoundRunner(tm, progress, out_dir, remote_root, cfg)
        try:
            rec = runner.run()
        except Exception:  # noqa: BLE001  ★失败留证据:先尽力回收再原异常非零中止
            progress.event("验收轮异常,尽力回收证据后中止", label)
            try:
                ev = runner.collect()
                idx["rounds"].append({"label": label, "window": w,
                                      "drive_exit": None,
                                      "error": "round raised; evidence salvaged",
                                      "evidence": ev})
                index_save(out_dir, idx)
            except Exception as exc2:  # noqa: BLE001
                progress.event("异常回收失败(证据仍在远端 rundir)",
                               f"{label}: {type(exc2).__name__}: {exc2}")
            raise
        rec.update({"window": w, "env_extra": {}, "knobs": {},
                    "scope": "G1 验收单流(peer.sh 多流/UDP/延迟/并发为诊断附带,"
                             "绝不混为单流)"})
        idx["rounds"].append(rec)
        index_save(out_dir, idx)
        if rec["drive_exit"] != 0:
            die(f"验收轮真实失败(label={label} exit={rec['drive_exit']},"
                f"证据已回收,按约定停止)")
        progress.event("验收窗口完成", label)
    idx["matrix_complete"] = True
    index_save(out_dir, idx)
    # 诊断轮:独立一轮,证据/采样置于证据根之外(不进验收样本)
    if args.diagnostics:
        label = "g1-tcp-nomux-diag"
        cfg = cc.RoundCfg(G1_PROTO, G1_MUX, label,
                          f"g1-diag-tcp-mux0-{int(time.time())}",
                          repeat=repeat, duration=duration,
                          conc_levels=G1_CONC_LEVELS)
        runner = G1RoundRunner(tm, progress, out_dir, remote_root, cfg,
                               evidence_root=os.path.join(out_dir, "diagnostics"),
                               diag_mode=True)
        diag_rec = {"label": label}
        try:
            cc.preflight_round(tm)
            runner.start_server()
            runner.start_client()
            rc = runner.run_drive_with_diag()
            diag_rec["drive_exit"] = rc
            diag_rec["strace"] = runner.strace_probe()
            diag_rec["diagnostics"] = runner.collect_diag()
        except Exception:  # noqa: BLE001  诊断失败也先回收诊断产物再非零中止
            progress.event("诊断轮异常,尽力回收诊断产物后中止", label)
            try:
                diag_rec.setdefault("diagnostics", runner.collect_diag())
                idx["diagnostics_rounds"].append(diag_rec)
                index_save(out_dir, idx)
            except Exception as exc2:  # noqa: BLE001
                progress.event("诊断异常回收失败(产物仍在远端 rundir)",
                               f"{type(exc2).__name__}: {exc2}")
            raise
        finally:
            runner.stop_all()
        idx["diagnostics_rounds"].append(diag_rec)
        index_save(out_dir, idx)
        progress.event("诊断轮完成", f"{label}(证据在 diagnostics/,验收样本未混入)")
    idx["scope"] = ("G1-only:TCP mux=0 relay=splice 每方向单流 10s×≥3 配对×"
                    f"{windows} 独立窗口;不宣称 G2-G7")
    index_save(out_dir, idx)
    progress.event("measure 完成", out_dir)
    return 0


# ================================================================ verify

VERDICT_KEYS = ("schema", "evidence_valid", "g1_pass", "regressions_pass",
                "upload_mbps", "download_mbps", "failures", "evidence_paths")


def build_verdict(run_dir: str, repo: str, require_e2e: bool = False) -> dict:
    """read-only 判定:返回 g1-tcp-verdict-v1 dict。绝不抛异常污染 stdout。"""
    failures: list[str] = []
    evidence_paths: list[str] = []
    upload = download = None
    evidence_valid = True
    regressions_pass = True
    g1_pass = False

    def fail(msg: str, structural: bool = True):
        nonlocal evidence_valid
        failures.append(msg)
        if structural:
            evidence_valid = False

    run_dir = os.path.abspath(run_dir)
    repo = os.path.abspath(repo)
    idx_path = os.path.join(run_dir, "run-index.json")
    evidence_paths.append(idx_path)
    logs_dir = os.path.join(run_dir, "logs")
    ev_root = os.path.join(run_dir, "evidence")

    idx = None
    if not os.path.isfile(idx_path):
        fail(f"缺 run-index.json(不是 g1_tcp 产物目录): {run_dir}")
    else:
        try:
            idx = json.load(open(idx_path, encoding="utf-8"))
        except (OSError, ValueError) as exc:
            fail(f"run-index.json 不可解析: {exc}")
            idx = None
        else:
            if idx.get("schema") != SCHEMA:
                fail(f"run-index schema 不符: {idx.get('schema')!r}(期望 {SCHEMA})")
    if idx is not None and idx.get("tree_hash"):
        # declared tree 绑定:TREE 构建输入哈希必须与记录一致(当前源码变化→FAIL)
        if not os.path.isdir(repo):
            fail(f"--repo 不是目录: {repo}")
        else:
            try:
                live = cc.tree_hash(repo)
            except OSError as exc:
                fail(f"declared tree 哈希不可算: {exc}")
            else:
                if live != idx["tree_hash"]:
                    fail("declared tree 与记录 tree_hash 不一致(源码在测量后被改动,"
                         f"或 --repo 不是测量源):live={live[:16]}… vs "
                         f"recorded={str(idx['tree_hash'])[:16]}…")

    # ---- A. 真实 Linux test/build log + exit(原件)----------------------
    tw_exit_f = os.path.join(logs_dir, "test-workspace.exit")
    tw_log_f = os.path.join(logs_dir, "test-workspace.log")
    br_exit_f = os.path.join(logs_dir, "build-release.exit")
    for p in (tw_exit_f, tw_log_f, br_exit_f):
        if os.path.isfile(p):
            evidence_paths.append(p)
    if not (os.path.isfile(tw_exit_f) and os.path.isfile(tw_log_f)):
        fail("缺 test-workspace.log/.exit 原件(Linux workspace 测试真实 exit 不可验)")
        regressions_pass = False
    else:
        tw_exit = open(tw_exit_f, encoding="utf-8").read().strip()
        tw_log = open(tw_log_f, encoding="utf-8").read()
        summ = cc._parse_test_summary(tw_log)
        if tw_exit != "0" or summ["suites"] == 0 or summ["non_ok"]:
            fail(f"cargo test --workspace 非通过: exit={tw_exit} "
                 f"suites={summ['suites']} non_ok={summ['non_ok'][:3]}")
            regressions_pass = False
    if not os.path.isfile(br_exit_f):
        fail("缺 build-release.exit 原件(release 构建真实 exit 不可验)")
        regressions_pass = False
    elif open(br_exit_f, encoding="utf-8").read().strip() != "0":
        fail(f"cargo build --release 非零: "
             f"{open(br_exit_f, encoding='utf-8').read().strip()}")
        regressions_pass = False

    # ---- B. 证据目录:未改的 verify_goal.EvidenceDir(固定 RH_VERIFY_REPO)--
    evid = []
    if os.path.isdir(ev_root):
        dirs, _skipped = vg.discover_evidence_dirs(ev_root)
        for p in dirs:
            evidence_paths.append(p)
        # 固定显式 TREE(证据自带 repo 不回退);进程内还原,不污染同进程
        # 后续核验(单测 discovery 下 test_verify_goal* 共用一个解释器)
        prev_repo = os.environ.get("RH_VERIFY_REPO")
        os.environ["RH_VERIFY_REPO"] = repo
        try:
            for p in dirs:
                try:
                    d = vg.EvidenceDir(p)
                except Exception as exc:  # noqa: BLE001  fail closed
                    fail(f"证据目录解析异常 {os.path.basename(p)}: "
                         f"{type(exc).__name__}: {exc}")
                    continue
                evid.append(d)
            vg.cross_dir_independence(evid)
        finally:
            if prev_repo is None:
                os.environ.pop("RH_VERIFY_REPO", None)
            else:
                os.environ["RH_VERIFY_REPO"] = prev_repo
        for d in evid:
            if d.issues:
                for msg in d.issues:
                    fail(f"[{d.name}] {msg}")
            if d.config != (G1_PROTO, False, G1_RELAY):
                fail(f"[{d.name}] 非 G1 配置 {d.config}(期望 tcp/mux=false/splice)")
            if d.invalid_records:
                fail(f"[{d.name}] 存在 {len(d.invalid_records)} 条 invalid/失败样本"
                     "(验收轮不允许静默过滤;drive 非零即中止)")
    else:
        fail(f"缺证据根: {ev_root}")

    # ---- C. 轮次完整性与失败留痕(run-index ↔ 证据目录互证)--------------
    rec_labels = set()
    if idx is not None:
        rounds = idx.get("rounds") or []
        if not rounds:
            fail("run-index 无验收轮记录")
        for r in rounds:
            rec_labels.add(r.get("label"))
            if r.get("drive_exit") != 0:
                fail(f"验收轮 drive_exit={r.get('drive_exit')}(label="
                     f"{r.get('label')};runner 退出码不用于性能判定,但非零必须失败)")
            if r.get("env_extra") or r.get("knobs"):
                fail(f"验收轮携带环境开关(label={r.get('label')},"
                     f"env_extra={r.get('env_extra')}):最终成绩不得依赖非默认开关")
        ok_names = {d.name for d in evid if not d.issues}
        missing = rec_labels - ok_names
        if evid and missing:
            fail(f"证据目录缺失(轮记录有、目录无,疑似删除失败样本): {sorted(missing)}")
        # 诊断臂必须在证据根之外
        for r in (idx.get("diagnostics_rounds") or []):
            dpath = os.path.join(run_dir, "diagnostics", str(r.get("label")))
            if os.path.isdir(dpath):
                evidence_paths.append(dpath)

    # ---- D. G1 gate:evaluate_gates 返回的 G1 Gate 对象(非 CLI 退出码)----
    good = [d for d in evid if not d.issues and d.config == (G1_PROTO, False, G1_RELAY)]
    if good:
        gates = vg.evaluate_gates(evid)
        g1 = next((g for g in gates if g.gid == "G1"), None)
        if g1 is None:
            fail("evaluate_gates 未返回 G1 gate")
        else:
            g1_pass = bool(g1.ok)
            if not g1.ok:
                for det in g1.details:
                    failures.append(f"[G1] {det}")
            pairs = vg.collect_pairs(evid)
            for direction in ("upload", "download"):
                items = pairs.get((G1_PROTO, False, G1_RELAY, direction,
                                   G1_STREAMS), [])
                med = vg.median([i["tunnel"] for i in items])
                if direction == "upload":
                    upload = med
                else:
                    download = med
                # ≥3 独立配对显式复核(evaluate_gates 已含;这里落结构性失败)
                if len(items) < vg.THRESH["g7_min_pairs"]:
                    fail(f"{direction}: 有效配对 {len(items)} < "
                         f"{vg.THRESH['g7_min_pairs']}(G7)")
            # 单流方向目标:G1 配对键固定 streams=1;4/16 流为诊断附带,绝不混为单流
            # (raw 的方向/目标/流数一致性由 EvidenceDir._check_raw_shape 逐条核对)
            # 环境上限退出准则(独立复核修正1):fresh direct 下行中位仍 <3000
            dl_items = pairs.get((G1_PROTO, False, G1_RELAY, "download",
                                  G1_STREAMS), [])
            dl_direct = vg.median([i["direct"] for i in dl_items])
            dl_windows = len({i["dir_path"] for i in dl_items})
            if dl_direct is not None and dl_direct < ENV_LIMIT_DIRECT_MBPS:
                if (len(dl_items) >= ENV_LIMIT_MIN_PAIRS
                        and dl_windows >= ENV_LIMIT_MIN_WINDOWS):
                    failures.append(
                        f"[G1][环境受限判定·成立] fresh direct 下行中位 {dl_direct} < "
                        f"{ENV_LIMIT_DIRECT_MBPS} Mbps(n={len(dl_items)},"
                        f"{dl_windows} 独立窗口)→ G1-下行按当前三机环境判"
                        f"'环境受限 FAIL':停止候选迭代,保留 default;上行 direct "
                        f"余量与此并存,软件差距(G6 retention)另行归因")
                else:
                    failures.append(
                        f"[G1][环境受限判定·待判] direct 下行中位 {dl_direct} < "
                        f"{ENV_LIMIT_DIRECT_MBPS},但独立窗口 {dl_windows} < "
                        f"{ENV_LIMIT_MIN_WINDOWS}(或配对 {len(dl_items)} < "
                        f"{ENV_LIMIT_MIN_PAIRS}):环境受限判定不成立,按普通 FAIL "
                        f"处理,不得据此继续无限候选迭代")
    else:
        fail("没有合格的 (tcp, mux=false, relay=splice) 证据目录")

    # ---- E. E2E(--require-e2e:四套原件实际 PASS)------------------------
    e2e_present = 0
    for key, _script in cc.E2E_SUITES:
        exit_f = os.path.join(logs_dir, f"e2e-{key}.exit")
        log_f = os.path.join(logs_dir, f"e2e-{key}.log")
        have = os.path.isfile(exit_f) and os.path.isfile(log_f)
        if have:
            e2e_present += 1
            evidence_paths.append(exit_f)
            evidence_paths.append(log_f)
        if not have and not require_e2e:
            continue
        if not have:
            fail(f"--require-e2e 缺 E2E 原件: {key}", structural=False)
            regressions_pass = False
            continue
        exit_v = open(exit_f, encoding="utf-8").read().strip()
        log_txt = open(log_f, encoding="utf-8").read()
        m = re.findall(r"PASS=(\d+) FAIL=(\d+)", log_txt)
        pairs_pf = [list(map(int, x)) for x in m]
        ok = exit_v == "0" and pairs_pf and all(f == 0 for _, f in pairs_pf) \
            and any(p > 0 for p, _ in pairs_pf)
        if not ok:
            fail(f"E2E {key} 原件未 PASS: exit={exit_v} PASS/FAIL={pairs_pf}",
                 structural=False)
            regressions_pass = False
    # ★独立复核 blocker 1:无/缺 E2E 原件且未设 --require-e2e 时,必须显式声明
    # regressions_pass 的覆盖面,防止被引用为四套 E2E 证据(非结构性注记)。
    if not require_e2e and e2e_present < len(cc.E2E_SUITES):
        failures.append(
            f"[E2E][未覆盖] E2E 原件 {e2e_present}/{len(cc.E2E_SUITES)} 且未设 "
            f"--require-e2e:本 regressions_pass 仅覆盖 cargo test --workspace "
            f"+ cargo build --release,不覆盖四套 E2E,不得作为 E2E 证据引用;"
            f"最终安全须以 measure --final 同构建执行四套 E2E 并以 "
            f"verify --require-e2e 复判")

    for name, val in (("upload_mbps", upload), ("download_mbps", download)):
        if val is not None:
            if not (isinstance(val, (int, float)) and val == val
                    and abs(val) != float("inf")):
                if name == "upload_mbps":
                    upload = None
                else:
                    download = None
    return {
        "schema": VERDICT_SCHEMA,
        "evidence_valid": bool(evidence_valid),
        "g1_pass": bool(g1_pass and evidence_valid),
        "regressions_pass": bool(regressions_pass),
        "upload_mbps": upload,
        "download_mbps": download,
        "failures": failures,
        "evidence_paths": sorted(set(evidence_paths)),
    }


def cmd_verify(args) -> int:
    verdict = build_verdict(args.run_dir, args.repo,
                            require_e2e=bool(args.require_e2e))
    # 判定留档:run-dir 的父目录若存在 gates/(prepare 根),写入只增不覆盖副本
    gates_dir = os.path.join(os.path.dirname(os.path.abspath(args.run_dir)), "gates")
    if os.path.isdir(gates_dir):
        overall = all((verdict["evidence_valid"], verdict["g1_pass"],
                       verdict["regressions_pass"]))
        name = (f"verdict-{os.path.basename(os.path.abspath(args.run_dir))}-"
                f"{_dt.datetime.now().strftime('%Y%m%d-%H%M%S')}-"
                f"{'PASS' if overall else 'FAIL'}.json")
        try:
            _write_new_file(os.path.join(gates_dir, name),
                            json.dumps(verdict, ensure_ascii=False, indent=2,
                                       sort_keys=True, allow_nan=False) + "\n")
        except SystemExit:
            print(f"[g1][warn] gates 留档失败(不覆盖既有): {name}", file=sys.stderr)
    # stdout 只输出单个严格 JSON(拒绝 NaN/Infinity)
    sys.stdout.write(json.dumps(verdict, ensure_ascii=False, allow_nan=False) + "\n")
    ok = verdict["evidence_valid"] and verdict["g1_pass"] and verdict["regressions_pass"]
    return 0 if ok else 1


# ================================================================ fixed-ab

def fixed_ab_find_example(tree: str) -> "str | None":
    p = os.path.join(tree, *EXAMPLE_REL.split("/"))
    return p if os.path.isfile(p) else None


def fixed_ab_prepare_trees(out_dir: str, cand_tree: str, base_tree: str) -> dict:
    """本地冻结两臂:candidate=候选确定性副本;baseline=基线快照副本+候选
    example 原样叠加(同计数同钩子;唯一差异是底层泵)。"""
    trees = os.path.join(out_dir, "trees")
    cand = os.path.join(trees, "candidate")
    base = os.path.join(trees, "baseline")
    for d in (cand, base):
        if os.path.exists(d):
            die(f"拒绝覆盖既有冻结树: {d}")
    cc.snapshot_workspace(cand_tree, cand)
    cc.snapshot_workspace(base_tree, base)
    ex_src = fixed_ab_find_example(cand_tree)
    if not ex_src:
        die(f"候选树缺 example: {EXAMPLE_REL}(fixed-ab 不可用)")
    ex_dst = os.path.join(base, *EXAMPLE_REL.split("/"))
    os.makedirs(os.path.dirname(ex_dst), exist_ok=True)
    shutil.copy2(ex_src, ex_dst)
    return {"candidate": cand, "baseline": base,
            "example_sha256": sha256_file(ex_src),
            "candidate_tree_hash": cc.tree_hash(cand),
            "baseline_tree_hash": cc.tree_hash(base),
            "note": "baseline=prepare baseline-source+候选 example 原样叠加"
                    "(同计数同钩子;release flags 一致,unset RUSTFLAGS)"}


def fixed_ab_summarize(results: list) -> dict:
    """★独立复核 blocker 2:汇总必须给出小包 p50/p99 的**全量口径**
    (median/min/max/n + 逐轮值),防止文档只引用单轮数字冒充全量;
    gbps 保留 median。results 元素含 round/arm/direction/gbps/small_packet。"""
    out = {}
    for direction in ("upload", "download"):
        for tag in ("old", "new"):
            items = [x for x in results
                     if x.get("direction") == direction and x.get("arm") == tag]
            gbps_vals = [x["gbps"] for x in items
                         if isinstance(x.get("gbps"), (int, float))]

            def _agg(key):
                vals = [x["small_packet"][key] for x in items
                        if isinstance(x.get("small_packet"), dict)
                        and isinstance(x["small_packet"].get(key), (int, float))]
                if not vals:
                    return None
                return {"median": vg.median(vals), "min": min(vals),
                        "max": max(vals), "n": len(vals)}
            out[f"{direction}-{tag}"] = {
                "median_gbps": vg.median(gbps_vals), "n_gbps": len(gbps_vals),
                "small_packet_p50_us": _agg("p50_us"),
                "small_packet_p99_us": _agg("p99_us"),
                "per_round": [{"round": x.get("round"), "gbps": x.get("gbps"),
                               "p50_us": (x.get("small_packet") or {}).get("p50_us"),
                               "p99_us": (x.get("small_packet") or {}).get("p99_us")}
                              for x in items]}
    return out


def cmd_fixed_ab(args) -> int:
    out_dir = os.path.abspath(args.out_dir)
    cand_tree = os.path.abspath(args.source) if args.source else REPO_ROOT
    ex = fixed_ab_find_example(cand_tree)
    if not ex:
        # 明确非零'不可用';绝不冒充三机达标
        print(f"[g1][fixed-ab] 不可用:候选树({cand_tree})缺 {EXAMPLE_REL}。\n"
              f"example CLI 契约见 {os.path.join(DEFAULT_ROOT, 'example-contract.md')}"
              f"(由 prepare 落盘);实现工程师按契约补齐后重跑。",
              file=sys.stderr)
        return 3
    if os.path.exists(out_dir) and os.listdir(out_dir):
        die(f"--out-dir 非空目录(新证据只增不覆盖): {out_dir}")
    os.makedirs(out_dir, exist_ok=True)
    progress = cc.Progress(out_dir)
    tm = cc.Tm(progress, os.path.join(out_dir, "logs"))
    stamp = _dt.datetime.now().strftime("%Y%m%d-%H%M%S")
    prepare_root = os.path.abspath(args.prepare_root)
    base_tree = (os.path.abspath(args.baseline) if args.baseline
                 else os.path.join(prepare_root, "baseline-source"))
    if not os.path.isdir(base_tree):
        die(f"fixed-ab 需要基线源(prepare 的 baseline-source 或 --baseline):"
            f"缺 {base_tree}")
    idx = index_load(out_dir)
    idx.update({"mode": "fixed-ab", "started_at": now_iso(),
                "candidate_source": cand_tree, "baseline_source": base_tree,
                "mib": int(args.mib), "rounds": int(args.rounds), **tool_shas()})
    index_save(out_dir, idx)
    cc._quiesce_check(cand_tree, progress)
    info = fixed_ab_prepare_trees(out_dir, cand_tree, base_tree)
    idx["fixed_ab_trees"] = info
    index_save(out_dir, idx)
    # 同步两臂到 test1 并构建(--example;unset RUSTFLAGS:两臂 release flags 一致)
    arms = {}
    toolchain = cc.remote_toolchain_pinned(info["candidate"]) or "stable"
    cc.remote_fd_preflight(tm, cc.ASSETS["test1"])
    for arm, tree in (("baseline", info["baseline"]), ("candidate", info["candidate"])):
        remote_root = f"/tmp/rh-g1-fixedab-{arm}-{stamp}"
        tar_path = os.path.join(out_dir, f"source-{arm}-{stamp}.tar")
        with tarfile.open(tar_path, "w") as tf:
            for rel in cc.enumerate_workspace_files(tree):
                tf.add(os.path.join(tree, rel), arcname=rel, recursive=False)
            if arm == "candidate":
                tf.add(os.path.abspath(__file__), arcname="benchmark/g1_tcp.py",
                       recursive=False)
        tar_sha = sha256_file(tar_path)
        a = cc.ASSETS["test1"]
        tm.exec_sh(a["id"], f"mkdir -p {remote_root}", name=f"mkdir-{arm}")
        tm.upload(a["id"], tar_path, f"{remote_root}/src.tar", name=f"ul-{arm}")
        got = tm.exec_sh(a["id"],
                         f"sha256sum {remote_root}/src.tar | awk '{{print $1}}'",
                         name=f"sha-{arm}").strip()
        if got != tar_sha:
            die(f"{arm} 源包哈希不一致: {got} != {tar_sha}")
        tm.exec_sh(a["id"], f"cd {remote_root} && rm -rf tree && mkdir tree && "
                            f"tar xf src.tar -C tree", name=f"untar-{arm}")
        pre = (f"export PATH=$HOME/.cargo/bin:$PATH; "
               f"export RUSTUP_TOOLCHAIN={shlex.quote(toolchain)}; "
               f"unset RUSTFLAGS; cd {remote_root}/tree && ")
        rc = cc.remote_nohup(
            tm, a, pre + "cargo build --release --locked --example g1_splice_bench",
            remote_root, f"build-{arm}", deadline=cc.DEADLINES["cargo_build"])
        if rc != 0:
            tail = tm.exec_sh(a["id"],
                              f"tail -40 {remote_root}/build-{arm}.log", check=False)
            die(f"fixed-ab {arm} 臂构建失败(exit={rc}):\n{tail}")
        bin_remote = f"{remote_root}/tree/target/release/examples/g1_splice_bench"
        bin_sha = tm.exec_sh(a["id"],
                             f"sha256sum {bin_remote} | awk '{{print $1}}'",
                             name=f"binsha-{arm}").strip()
        if not HEX64.match(bin_sha):
            die(f"{arm} 臂 example 二进制缺失/哈希异常: {bin_sha!r}")
        arms[arm] = {"remote_root": remote_root, "bin_sha256": bin_sha}
        idx["arms"] = arms
        index_save(out_dir, idx)
    # 交替对照:old(baseline)→new(candidate)×rounds,每臂双向固定工作量
    a = cc.ASSETS["test1"]
    results = []
    for r in range(1, int(args.rounds) + 1):
        for arm, tag in (("baseline", "old"), ("candidate", "new")):
            for direction in ("upload", "download"):
                out_json = f"{arms[arm]['remote_root']}/ab-r{r}-{tag}-{direction}.json"
                cmd = (f"cd {arms[arm]['remote_root']}/tree && "
                       f"ulimit -n 65535 2>/dev/null; "
                       f"target/release/examples/g1_splice_bench "
                       f"--direction {direction} --mib {int(args.mib)} --json "
                       f"> {out_json}; echo $? > {out_json}.exit")
                rc = cc.remote_nohup(tm, a, cmd, arms[arm]["remote_root"],
                                     f"ab-r{r}-{tag}-{direction}",
                                     deadline=cc.DEADLINES["drive_round"])
                local = os.path.join(out_dir, "fixed-ab",
                                     f"r{r}-{tag}-{direction}.json")
                os.makedirs(os.path.dirname(local), exist_ok=True)
                tm.download(a["id"], out_json, local,
                            name=f"dl-ab-r{r}-{tag}-{direction}")
                tm.download(a["id"], out_json + ".exit", local + ".exit",
                            name=f"dl-ab-exit-r{r}-{tag}-{direction}")
                # ★独立复核 H3:nohup 包装日志(臂进程 stdout/stderr,含候选臂
                # 泵统计)此前只留在远端未回收——必须一并回收为 .runlog 原件。
                try:
                    tm.download(a["id"], out_json.replace(".json", ".log"),
                                local + ".runlog",
                                name=f"dl-ab-log-r{r}-{tag}-{direction}")
                except (cc.TmError, cc.CommandTimeout) as exc:
                    print(f"[g1][warn] 臂运行日志回收失败(r={r} {tag}/{direction}):"
                          f" {exc}", file=sys.stderr)
                if rc != 0:
                    die(f"fixed-ab 臂失败(r={r} {tag}/{direction} exit={rc};"
                        f"输出已回收,非零中止)")
                try:
                    doc = json.loads(open(local, encoding="utf-8").read())
                except (OSError, ValueError) as exc:
                    die(f"fixed-ab 输出不可解析(r={r} {tag}/{direction}): {exc}")
                if doc.get("schema") != "fixed-ab-v1":
                    die(f"fixed-ab 输出 schema 不符(r={r} {tag}/{direction}): "
                        f"{doc.get('schema')!r}(契约见 example-contract.md)")
                results.append({"round": r, "arm": tag, "direction": direction,
                                "path": os.path.relpath(local, out_dir),
                                "gbps": doc.get("gbps"),
                                "small_packet": doc.get("small_packet")})
                idx["fixed_ab_results"] = results
                index_save(out_dir, idx)
    # 汇总(只归因;不设门槛、不冒充三机 G1 达标;小包为全量口径防单轮引用)
    summary = fixed_ab_summarize(results)
    _write_new_file(os.path.join(out_dir, "fixed-ab", "summary.json"),
                    json.dumps({"created_at": now_iso(), "summary": summary,
                                "results": results, "mib": int(args.mib),
                                "disclaimer": "固定工作量 Linux 对照仅归因,"
                                              "不构成三机 G1 达标判定;"
                                              "small_packet_* 为全量口径"
                                              "(median/min/max,n),"
                                              "不得引用单轮值冒充全量"},
                               ensure_ascii=False, indent=2, sort_keys=True) + "\n")
    idx["fixed_ab_summary"] = summary
    index_save(out_dir, idx)
    progress.event("fixed-ab 完成", json.dumps(summary, ensure_ascii=False))
    return 0


# ================================================================ 内部子命令

_SALVAGE_TICK_PAT = re.compile(r"^(\d+)-diag-tick-(server|client)-(\d+)\.log$")


def _extract_termark_output(log_path: str):
    """从 termark 调用日志文件提取 JSON output 字段。

    ★不能直接用 cc.Tm._parse(文件):日志首行是 `$ termark ... <cmd>`,我的
    tick 命令含 awk `{print ...}` 花括号,find('{') 会命中命令行而非 JSON
    起点 → 解析失败(实时路径无此问题:它解析的是纯 stdout)。这里从第一个
    整行 `{`(termark pretty JSON 起点)开始取块解析。
    """
    text = open(log_path, encoding="utf-8", errors="replace").read()
    lines = text.splitlines()
    for i, ln in enumerate(lines):
        if ln.strip() == "{":
            try:
                obj = json.loads("\n".join(lines[i:]))
            except ValueError:
                return None
            if isinstance(obj, dict) and "output" in obj:
                return obj["output"]
            return None
    return None


def cmd_internal_salvage_diag(args) -> int:
    """从本地 termark 调用原件(logs/NNNN-diag-tick-<host>-N.log 的 JSON output)
    恢复诊断 tick 的完整 ss/proc 输出到 diagnostics/<label>/-recovered.log。

    ★round-4 事故:tick 重定向 bug 使远端 diag-ss-proc.log 只落了 pipe_max
    一行/tick,但完整输出被 termark 通道日志如实留痕(本地原件)。本子命令
    只读本地原件、只增不覆盖,不做任何远端操作。
    """
    run_dir = os.path.abspath(args.run_dir)
    logs = os.path.join(run_dir, "logs")
    if not os.path.isdir(logs):
        die(f"缺 logs 目录: {logs}")
    idx_path = os.path.join(run_dir, "run-index.json")
    idx = json.load(open(idx_path, encoding="utf-8")) \
        if os.path.isfile(idx_path) else {}
    label = args.label or ((idx.get("diagnostics_rounds") or [{}])[0].get("label"))
    if not label:
        die(f"无法确定诊断轮 label(--label 显式指定): {run_dir}")
    per_host: dict[str, list] = {"server": [], "client": []}
    for name in sorted(os.listdir(logs)):
        m = _SALVAGE_TICK_PAT.match(name)
        if not m:
            continue
        out = _extract_termark_output(os.path.join(logs, name))
        if not isinstance(out, str):
            out = f"# [salvage] {name} 无可解析 output 字段"
        per_host[m.group(2)].append((int(m.group(1)), int(m.group(3)), out))
    ddir = os.path.join(run_dir, "diagnostics", label)
    os.makedirs(ddir, exist_ok=True)
    total = 0
    for host in ("server", "client"):
        items = per_host[host]
        if not items:
            continue
        dst = os.path.join(ddir, f"{host}-diag-ss-proc-recovered.log")
        body = [f"# derived {now_iso()} from logs/*-diag-tick-{host}-*.log"
                f"(termark 通道原件;重定向 bug 期间完整输出仅存于此)\n"]
        for seq, tick, out in sorted(items):
            body.append(f"\n# ===== recovered from logs/"
                        f"{seq:04d}-diag-tick-{host}-{tick}.log =====\n{out}")
        _write_new_file(dst, "".join(body))
        total += len(items)
        print(f"[g1] salvage {host}: {len(items)} ticks → {dst}")
    if total == 0:
        die(f"未找到任何 diag-tick 原件: {logs}")
    return 0


# ================================================================ self-test

def _fx_run(root: str, samples: int = 3, tunnel_mbps: float = 3500.0,
            direct_mbps: float = 10000.0, windows: int = 1, e2e: bool = True,
            mode: str = "final", subdir: str = "run") -> tuple:
    """合成一个 g1_tcp measure 产物目录 + 对应 fx repo,返回 (run_dir, repo)。"""
    repo = vg._fx_repo(root)
    run = os.path.join(root, subdir)
    ev_root = os.path.join(run, "evidence")
    os.makedirs(ev_root, exist_ok=True)
    for w in range(1, windows + 1):
        vg.build_fixture_dir(ev_root, f"tcp-nomux-w{w}", "tcp", False, "splice",
                             samples=samples, tunnel_mbps=tunnel_mbps,
                             direct_mbps=direct_mbps,
                             base_us=vg.FIX_BASE_US + w * 3_600_000_000)
    logs = os.path.join(run, "logs")
    os.makedirs(logs, exist_ok=True)
    with open(os.path.join(logs, "test-workspace.exit"), "w") as fh:
        fh.write("0\n")
    with open(os.path.join(logs, "test-workspace.log"), "w") as fh:
        fh.write("test result: ok. 42 passed; 0 failed\n")
    with open(os.path.join(logs, "build-release.exit"), "w") as fh:
        fh.write("0\n")
    if e2e:
        for k, _ in cc.E2E_SUITES:
            with open(os.path.join(logs, f"e2e-{k}.exit"), "w") as fh:
                fh.write("0\n")
            with open(os.path.join(logs, f"e2e-{k}.log"), "w") as fh:
                fh.write("PASS: fx-a\n\n== 结果: PASS=10 FAIL=0 ==\n")
    idx = {"schema": SCHEMA, "version": SCHEMA_VERSION, "mode": mode,
           "tree_hash": cc.tree_hash(repo), "embedded_ui": False,
           "rounds": [{"label": f"tcp-nomux-w{w}", "proto": "tcp", "mux": 0,
                       "drive_exit": 0, "env_extra": {}, "window": w}
                      for w in range(1, windows + 1)],
           "diagnostics_rounds": []}
    index_save(run, idx)
    return run, repo


def cmd_self_test() -> int:
    import tempfile

    def expect(name, fn):
        try:
            got = fn()
        except Exception as exc:  # noqa: BLE001
            print(f"  [{name}] 异常: {type(exc).__name__}: {exc}")
            got = False
        ok = bool(got)
        print(f"  [{'ok' if ok else 'BAD'}] {name}")
        return ok

    checks = []

    def _v(run, repo, **kw):
        return build_verdict(run, repo, **kw)

    # 正例:双窗口完好装置 → 全过,严格 schema
    def _good():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, windows=2)
            v = _v(run, repo, require_e2e=True)
            return (v["evidence_valid"] and v["g1_pass"] and v["regressions_pass"]
                    and v["upload_mbps"] == 3500 and v["download_mbps"] == 3500
                    and not v["failures"]
                    and sorted(v.keys()) == sorted(VERDICT_KEYS)
                    and v["schema"] == VERDICT_SCHEMA)
    checks.append(expect("正例:完好装置双窗口 G1 通过(严格 schema)", _good))

    # 负例:缺 download 方向 → G1 FAIL
    def _missing_dir():
        import glob
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            d = os.path.join(run, "evidence", "tcp-nomux-w1")
            f = sorted(glob.glob(os.path.join(d, "results-*.jsonl")))[0]
            rows = [json.loads(x) for x in open(f, encoding="utf-8") if x.strip()]
            keep = [r for r in rows if not (r.get("metric") == "throughput"
                                            and r.get("dir") == "download")]
            with open(f, "w", encoding="utf-8") as fh:
                for r in keep:
                    fh.write(json.dumps(r) + "\n")
            v = _v(run, repo)
            return (not v["g1_pass"] and v["download_mbps"] is None)
    checks.append(expect("负例:缺 download 方向 → G1 FAIL", _missing_dir))

    # 负例:有效证据但吞吐低(2900<3000)→ g1_pass FAIL,evidence_valid 保持 True
    # (有效证据但吞吐低为 FAIL,非环境无效;direct 健康 → 无环境受限判定)
    def _low():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, tunnel_mbps=2900.0, windows=2)
            v = _v(run, repo)
            return (v["evidence_valid"] and not v["g1_pass"]
                    and v["upload_mbps"] == 2900
                    and not any("环境受限" in x for x in v["failures"]))
    checks.append(expect("负例:单流 2900<3000 → g1_pass FAIL(证据仍有效)", _low))

    # 负例:direct 下行也 <3000(≥3对、≥2 独立窗口)→ 环境受限 FAIL 判定出现
    def _env_limit():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, tunnel_mbps=2900.0, direct_mbps=2900.0,
                                windows=2)
            v = _v(run, repo)
            return (not v["g1_pass"]
                    and any("环境受限判定·成立" in x for x in v["failures"]))
    checks.append(expect("负例:direct 下行<3000(2 窗口)→ 环境受限 FAIL 判定",
                         _env_limit))

    # 负例:不足 3 对(samples=2)→ FAIL
    def _two_pairs():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, samples=2)
            v = _v(run, repo)
            return (not v["g1_pass"] and not v["evidence_valid"])
    checks.append(expect("负例:配对 2<3 → FAIL", _two_pairs))

    # 负例:runner=0 ≠ 性能达标(drive 全 0 + 低吞吐仍是 g1 FAIL)
    def _runner_zero_not_pass():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, tunnel_mbps=2900.0)
            idx = json.load(open(os.path.join(run, "run-index.json"),
                                 encoding="utf-8"))
            assert all(r["drive_exit"] == 0 for r in idx["rounds"])
            v = _v(run, repo)
            return (v["evidence_valid"] and not v["g1_pass"])
    checks.append(expect("负例:runner(drive)全 0 ≠ 性能达标", _runner_zero_not_pass))

    # 负例:失败样本静默混入(invalid 记录 + drive_exit 仍写 0)→ evidence_valid FAIL
    def _invalid_kept():
        import glob
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            d = os.path.join(run, "evidence", "tcp-nomux-w1")
            f = sorted(glob.glob(os.path.join(d, "results-*.jsonl")))[0]
            rows = [json.loads(x) for x in open(f, encoding="utf-8") if x.strip()]
            for r in rows:
                if (r.get("metric") == "throughput" and r.get("path") == "direct"
                        and r.get("streams") == 1 and r.get("dir") == "upload"):
                    r["valid"] = False
                    r["mbps"] = None
                    r["exit"] = 1
                    r["error"] = {"iperf_exit": 1, "stderr_tail": "fx fail"}
                    rid = r.get("raw_exit", "").split("/")[-1]
                    open(os.path.join(d, "raw", rid), "w").write("1\n")
                    break
            with open(f, "w", encoding="utf-8") as fh:
                for r in rows:
                    fh.write(json.dumps(r) + "\n")
            v = _v(run, repo)
            return (not v["evidence_valid"]
                    and any("invalid/失败样本" in x for x in v["failures"]))
    checks.append(expect("负例:失败样本(invalid)不可静默过滤 → FAIL", _invalid_kept))

    # 负例:当前源码变化(declared tree 与记录 tree_hash 不一致)→ FAIL
    def _src_changed():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            with open(os.path.join(repo, "crates", "synthetic", "a.rs"), "a") as fh:
                fh.write("// touched after measure\n")
            v = _v(run, repo)
            return (not v["evidence_valid"]
                    and any("tree_hash 不一致" in x for x in v["failures"]))
    checks.append(expect("负例:当前源码变化 → declared tree 绑定 FAIL", _src_changed))

    # 负例:旧二进制(endpoint exe_sha256 与 binaries.json 不一致)→ FAIL
    def _old_bin():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            p = os.path.join(run, "evidence", "tcp-nomux-w1", "endpoints",
                             "client", "endpoint.json")
            doc = json.load(open(p, encoding="utf-8"))
            doc["identity"]["exe_sha256"] = "0" * 64
            json.dump(doc, open(p, "w", encoding="utf-8"))
            v = _v(run, repo)
            return not v["evidence_valid"]
    checks.append(expect("负例:旧二进制(endpoint≠binaries.json)→ FAIL", _old_bin))

    # 负例:错 raw 目标(tunnel 记录 raw connected 是直连目标)→ FAIL
    def _raw_target():
        import glob
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            d = os.path.join(run, "evidence", "tcp-nomux-w1")
            f = sorted(glob.glob(os.path.join(d, "results-*.jsonl")))[0]
            recs = [json.loads(x) for x in open(f, encoding="utf-8") if x.strip()]
            victim = next(r for r in recs
                          if r.get("metric") == "throughput"
                          and r.get("path") == "tunnel" and r.get("streams") == 1)
            rawp = os.path.join(d, victim["raw_json"])
            raw = json.load(open(rawp, encoding="utf-8"))
            raw["start"]["connected"][0] = {"remote_host": "10.0.0.2",
                                            "remote_port": 19101}
            json.dump(raw, open(rawp, "w", encoding="utf-8"))
            v = _v(run, repo)
            return not v["evidence_valid"]
    checks.append(expect("负例:错 raw 目标(隧道记录实跑直连)→ FAIL", _raw_target))

    # 负例:--require-e2e 缺 E2E 原件 → regressions_pass FAIL(g1 不受影响)
    def _no_e2e():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, e2e=False)
            v = _v(run, repo, require_e2e=True)
            return (not v["regressions_pass"] and v["g1_pass"]
                    and v["evidence_valid"])
    checks.append(expect("负例:--require-e2e 缺四套原件 → regressions FAIL", _no_e2e))

    # 负例:E2E 原件真实失败 → regressions_pass FAIL
    def _bad_e2e():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            lg = os.path.join(run, "logs")
            open(os.path.join(lg, "e2e-transports.exit"), "w").write("1\n")
            open(os.path.join(lg, "e2e-transports.log"), "w").write(
                "PASS: a\nFAIL: b\n\n== 结果: PASS=3 FAIL=2 ==\n")
            v = _v(run, repo, require_e2e=True)
            return (not v["regressions_pass"] and v["evidence_valid"])
    checks.append(expect("负例:E2E 原件真实失败 → regressions FAIL", _bad_e2e))

    # 负例:CPU 覆盖不足 → G1 FAIL(方向判定不因上行掩盖)
    def _cpu_cov():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            p = os.path.join(run, "evidence", "tcp-nomux-w1", "endpoints",
                             "client", "cpu.jsonl")
            lines = open(p, encoding="utf-8").read().splitlines()
            with open(p, "w", encoding="utf-8") as fh:
                fh.write("\n".join(lines[:6]) + "\n")
            v = _v(run, repo)
            return not v["g1_pass"]
    checks.append(expect("负例:CPU 覆盖率不足 → G1 FAIL", _cpu_cov))

    # 负例:非法 JSON(NaN 坏行)→ evidence_valid FAIL
    def _nan():
        import glob
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            f = sorted(glob.glob(os.path.join(run, "evidence", "tcp-nomux-w1",
                                              "results-*.jsonl")))[0]
            text = open(f, encoding="utf-8").read().replace(
                '"mbps": 3500', '"mbps": NaN', 1)
            if '"mbps": NaN' not in text:
                text = text.replace('"mbps":3500', '"mbps":NaN', 1)
            with open(f, "w", encoding="utf-8") as fh:
                fh.write(text)
            v = _v(run, repo)
            return (not v["evidence_valid"]
                    and any("坏行" in x for x in v["failures"]))
    checks.append(expect("负例:results 含 NaN 非法 JSON → FAIL", _nan))

    # 负例:验收轮携带环境开关(非默认开关冒充默认)→ FAIL
    def _knob():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            idx = index_load(run)
            idx["rounds"][0]["env_extra"] = {"RH_FAKE_KNOB": "1"}
            index_save(run, idx)
            v = _v(run, repo)
            return (not v["evidence_valid"]
                    and any("环境开关" in x for x in v["failures"]))
    checks.append(expect("负例:验收轮携带环境开关 → FAIL", _knob))

    # 负例:失败轮次(drive_exit=1)→ FAIL(失败留痕并非零)
    def _drive_fail():
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            idx = index_load(run)
            idx["rounds"][0]["drive_exit"] = 1
            index_save(run, idx)
            v = _v(run, repo)
            return (not v["evidence_valid"]
                    and any("drive_exit=1" in x for x in v["failures"]))
    checks.append(expect("负例:验收轮 drive_exit=1 → FAIL", _drive_fail))

    # prepare:不覆盖 + gates/ + HEAD/status + 修改基线 + 快照即当前未提交树
    def _prepare(tmp):
        import types
        root = os.path.join(tmp, "root")
        rc = cmd_prepare(types.SimpleNamespace(out_dir=root))
        return rc, root

    def _prepare_ok():
        with tempfile.TemporaryDirectory() as td:
            rc, root = _prepare(td)
            return (rc == 0
                    and os.path.isdir(os.path.join(root, "gates"))
                    and os.path.isfile(os.path.join(root, "HEAD.txt"))
                    and os.path.isfile(os.path.join(root, "git-status.txt"))
                    and os.path.isfile(os.path.join(root,
                                                    "baseline-modifications.json"))
                    and os.path.isdir(os.path.join(root, "baseline-source"))
                    and os.path.isfile(os.path.join(root, "example-contract.md"))
                    and os.path.isfile(os.path.join(root, "baseline-source",
                                                    "Cargo.toml")))
    checks.append(expect("prepare:目录/快照/基线/契约齐全", _prepare_ok))

    def _prepare_twice():
        import types
        with tempfile.TemporaryDirectory() as td:
            _prepare(td)
            root = os.path.join(td, "root")
            try:
                cmd_prepare(types.SimpleNamespace(out_dir=root))
                return False
            except SystemExit:
                return os.path.isfile(os.path.join(root, "prepared.json"))
    checks.append(expect("prepare:重复执行不覆盖(非零中止)", _prepare_twice))

    def _prepare_junk():
        import types
        with tempfile.TemporaryDirectory() as td:
            root = os.path.join(td, "root")
            os.makedirs(os.path.join(root, "junk"))
            try:
                cmd_prepare(types.SimpleNamespace(out_dir=root))
                return False
            except SystemExit:
                return True
    checks.append(expect("prepare:既有陌生内容拒绝(历史证据不覆盖)", _prepare_junk))

    # prepare 快照必须是当前未提交树(非 git archive HEAD):取一个已修改文件,
    # 快照内容==当前工作区内容且≠HEAD 版本
    def _prepare_uncommitted():
        with tempfile.TemporaryDirectory() as td:
            rc, root = _prepare(td)
            status_txt = open(os.path.join(root, "git-status.txt"),
                              encoding="utf-8").read()
            mod = next((ln[3:] for ln in status_txt.splitlines()
                        if ln.startswith(" M ")), None)
            if mod is None:
                return False           # 本工作区当前必有已修改文件
            snap = open(os.path.join(root, "baseline-source", mod), "rb").read()
            live = open(os.path.join(REPO_ROOT, mod), "rb").read()
            _rc, head_out, _err = cc.run_local(
                ["git", "-C", REPO_ROOT, "show", f"HEAD:{mod}"], 120)
            return (snap == live and snap != head_out
                    and os.path.isfile(os.path.join(root, "baseline-source",
                                                    "Cargo.toml")))
    checks.append(expect("prepare:快照含当前未提交树(非 git archive HEAD)",
                         _prepare_uncommitted))

    # prepare:契约文件已存在且逐字节一致 → 放行;被篡改 → 拒绝
    def _prepare_contract():
        import types
        with tempfile.TemporaryDirectory() as td:
            root = os.path.join(td, "root-a")
            os.makedirs(root)
            with open(os.path.join(root, "example-contract.md"), "w",
                      encoding="utf-8") as fh:
                fh.write(EXAMPLE_CONTRACT)
            try:
                cmd_prepare(types.SimpleNamespace(out_dir=root))
            except SystemExit:
                return False          # 一致契约必须放行
            root2 = os.path.join(td, "root-b")
            os.makedirs(root2)
            with open(os.path.join(root2, "example-contract.md"), "w",
                      encoding="utf-8") as fh:
                fh.write("# tampered\n")
            try:
                cmd_prepare(types.SimpleNamespace(out_dir=root2))
                return False          # 篡改契约必须拒绝(不覆盖)
            except SystemExit:
                return True
    checks.append(expect("prepare:契约一致放行/篡改拒绝", _prepare_contract))

    # fixed-ab:无 example → 非零不可用(不碰真实仓库、不建目录)
    def _fxab_missing():
        import types
        with tempfile.TemporaryDirectory() as td:
            fake = os.path.join(td, "cand-no-example")
            os.makedirs(fake)
            out = os.path.join(td, "out")
            args = types.SimpleNamespace(out_dir=out, source=fake,
                                         baseline=None, prepare_root=td,
                                         mib=FIXED_AB_MIB, rounds=1)
            rc = cmd_fixed_ab(args)
            return rc == 3 and not os.path.exists(out)
    checks.append(expect("fixed-ab:无 example → 非零(3)不可用,不建目录",
                         _fxab_missing))

    # fixed-ab:有 example 但缺基线源(prepare 未做)→ 非零中止
    def _fxab_no_baseline():
        import types
        with tempfile.TemporaryDirectory() as td:
            fake = os.path.join(td, "cand")
            exd = os.path.join(fake, *EXAMPLE_REL.split("/"))
            os.makedirs(os.path.dirname(exd), exist_ok=True)
            open(exd, "w").write("fn main(){}\n")
            args = types.SimpleNamespace(out_dir=os.path.join(td, "out"),
                                         source=fake, baseline=None,
                                         prepare_root=td, mib=8, rounds=1)
            try:
                cmd_fixed_ab(args)
                return False
            except SystemExit:
                return True
    checks.append(expect("fixed-ab:缺基线源(prepare 未做)→ 非零中止",
                         _fxab_no_baseline))

    # fixed-ab:树冻结(基线叠加 example,同钩子)
    def _fxab_trees():
        with tempfile.TemporaryDirectory() as td:
            base = os.path.join(td, "base")
            cand = os.path.join(td, "cand")
            for t in (base, cand):
                os.makedirs(os.path.join(t, "crates", "x", "src"))
                os.makedirs(os.path.join(t, "migrations"))
                open(os.path.join(t, "Cargo.toml"), "w").write("[workspace]\n")
                open(os.path.join(t, "crates", "x", "Cargo.toml"), "w").write("x\n")
                open(os.path.join(t, "crates", "x", "src", "lib.rs"), "w").write(
                    "fn a(){}\n")
                for m in cc.MIN_MIGRATIONS:
                    open(os.path.join(t, m), "w").write("--\n")
                open(os.path.join(t, "rust-toolchain.toml"), "w").write(
                    '[toolchain]\nchannel="fx"\n')
                os.makedirs(os.path.join(t, "benchmark"), exist_ok=True)
                open(os.path.join(t, "benchmark", "peer.sh"), "w").write("# p\n")
            # 基线与候选泵不同(旧/新),example 只在候选
            open(os.path.join(base, "crates", "x", "src", "lib.rs"), "a").write(
                "// old pump\n")
            open(os.path.join(cand, "crates", "x", "src", "lib.rs"), "a").write(
                "// new pump\n")
            exd = os.path.join(cand, *EXAMPLE_REL.split("/"))
            os.makedirs(os.path.dirname(exd), exist_ok=True)
            open(exd, "w").write("fn main(){}\n")
            info = fixed_ab_prepare_trees(td, cand, base)
            ex_in_base = os.path.join(info["baseline"], *EXAMPLE_REL.split("/"))
            return (os.path.isfile(ex_in_base)
                    and cc.sha256_file(ex_in_base) == info["example_sha256"]
                    and info["candidate_tree_hash"] != info["baseline_tree_hash"])
    checks.append(expect("fixed-ab:基线树叠加候选 example(同钩子)", _fxab_trees))

    # verify CLI:stdout 单个严格 JSON(拒绝 NaN;键固定)
    def _cli_json():
        import subprocess
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td)
            p = subprocess.run(
                [sys.executable, os.path.abspath(__file__), "verify",
                 "--run-dir", run, "--repo", repo, "--json", "--require-e2e"],
                capture_output=True, text=True, timeout=600)
            lines = [x for x in p.stdout.splitlines() if x.strip()]
            if p.returncode != 0 or len(lines) != 1:
                return False

            def _reject(tok):
                raise ValueError(f"非有限数 {tok}")
            doc = json.loads(lines[0], parse_constant=_reject)
            return (sorted(doc.keys()) == sorted(VERDICT_KEYS)
                    and doc["schema"] == VERDICT_SCHEMA
                    and doc["evidence_valid"] and doc["g1_pass"]
                    and doc["regressions_pass"])
    checks.append(expect("verify CLI:stdout 单个严格 JSON 且全过", _cli_json))

    # CLI 失败路径:低吞吐装置 → 非零退出 + JSON 如实 FAIL
    def _cli_fail():
        import subprocess
        with tempfile.TemporaryDirectory() as td:
            run, repo = _fx_run(td, tunnel_mbps=2900.0)
            p = subprocess.run(
                [sys.executable, os.path.abspath(__file__), "verify",
                 "--run-dir", run, "--repo", repo, "--json"],
                capture_output=True, text=True, timeout=600)
            doc = json.loads(p.stdout.strip().splitlines()[-1])
            return (p.returncode == 1 and doc["evidence_valid"]
                    and not doc["g1_pass"])
    checks.append(expect("verify CLI:低吞吐 → 非零退出且 JSON 如实 FAIL", _cli_fail))

    ok = all(checks)
    print(f"== g1_tcp self-test: {'PASS' if ok else 'FAIL'} "
          f"({sum(checks)}/{len(checks)}) ==")
    return 0 if ok else 1


# ---------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(
        description="G1 TCP nomux 单流专项验收装置(prepare/measure/verify/fixed-ab)",
        formatter_class=argparse.RawDescriptionHelpFormatter, epilog=__doc__)
    sub = ap.add_subparsers(dest="cmd")

    p = sub.add_parser("prepare", help="创建 ROOT:HEAD/status/修改基线/当前树快照/gates")
    p.add_argument("--out-dir", required=True)

    p = sub.add_parser("measure", help="冻结源→Linux 构建测试→三机 G1 单流配对测量")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--source", help="候选源树(缺省=冻结当前工作区)")
    p.add_argument("--diagnostics", action="store_true",
                   help="额外独立诊断轮(ss/proc/pipe/strace;证据置于证据根外)")
    p.add_argument("--final", action="store_true",
                   help="同构建完整执行并回收四套 E2E")
    p.add_argument("--repeat", type=int, default=3)
    p.add_argument("--duration", type=int, default=10)
    p.add_argument("--windows", type=int, default=2,
                   help="独立窗口轮数(每轮独立 server/client/drive 生命周期)")

    p = sub.add_parser("verify", help="G1-only 判定:stdout 单个严格 JSON")
    p.add_argument("--run-dir", required=True)
    p.add_argument("--repo", required=True, help="declared TREE(RH_VERIFY_REPO 固定)")
    p.add_argument("--json", action="store_true",
                   help="严格 JSON 输出(默认亦为 JSON;保留 CLI 兼容)")
    p.add_argument("--require-e2e", action="store_true",
                   help="四套 E2E 原件必须实际 PASS")

    p = sub.add_parser("fixed-ab", help="固定工作量 Linux 对照(基线/候选交替)")
    p.add_argument("--out-dir", required=True)
    p.add_argument("--source", help="候选源树(缺省=当前工作区)")
    p.add_argument("--baseline", help="基线源树(缺省=prepare 的 baseline-source)")
    p.add_argument("--prepare-root", default=DEFAULT_ROOT)
    p.add_argument("--mib", type=int, default=FIXED_AB_MIB)
    p.add_argument("--rounds", type=int, default=FIXED_AB_ROUNDS)

    p = sub.add_parser("_internal-salvage-diag", help=argparse.SUPPRESS)
    p.add_argument("--run-dir", required=True)
    p.add_argument("--label", default=None)

    ap.add_argument("--self-test", action="store_true",
                    help="本地对抗自检(不碰远端)")

    args = ap.parse_args()
    if args.self_test:
        return cmd_self_test()
    if not args.cmd:
        ap.print_help()
        return 2
    if args.cmd == "prepare":
        return cmd_prepare(args)
    if args.cmd == "measure":
        return cmd_measure(args)
    if args.cmd == "verify":
        return cmd_verify(args)
    if args.cmd == "fixed-ab":
        return cmd_fixed_ab(args)
    if args.cmd == "_internal-salvage-diag":
        return cmd_internal_salvage_diag(args)
    ap.print_help()
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
