"""g1_tcp 回归(标准库 unittest,本地运行,不碰远端、不跑 prepare/measure 于真实目录)。

锁定的关键面(全部 fail-closed,与任务书列出的负例一一对应):
  * verify 判定:缺方向 / 低门槛(有效证据但吞吐低=FAIL,非环境无效)/ 不足 3 对
    / 失败样本不可静默过滤 / 当前源码变化(declared tree 绑定)/ 旧二进制 /
    错 raw 目标 / CPU 覆盖不足 / 非法 JSON(NaN)/ 缺 E2E(--require-e2e)/
    验收轮携带环境开关 / drive 失败 / runner=0 ≠ 性能达标 / 环境受限判定;
  * stdout 严格 JSON:schema=g1-tcp-verdict-v1、键集固定、拒绝 NaN/Infinity、
    单个 JSON 对象;gates/ 留档只增不覆盖;
  * prepare:只创建不存在目录、重复/陌生内容拒绝、HEAD/status/修改基线/快照
    (当前未提交树,非 git archive HEAD)/gates//契约文件一致放行篡改拒绝;
  * fixed-ab:无 example 非零(3)不可用、缺基线源非零、基线树叠加候选
    example(同计数同钩子);
  * 诊断臂置于证据根之外,不污染验收判定。
不弱化既有 tests(本文件为新增;closure_c84/verify_goal 既有测试原样保留)。
"""
from __future__ import annotations

import contextlib
import glob
import io
import json
import os
import subprocess
import sys
import tempfile
import types
import unittest

sys.path.insert(0, __file__.rsplit("/", 1)[0])
import fixtures as fx  # noqa: E402  (约定共享基建:把 benchmark/py 挂上 sys.path)
sys.path.insert(0, fx.BENCH)      # g1_tcp.py / closure_c84.py 在 benchmark/ 下

import g1_tcp as g1  # noqa: E402
import closure_c84 as cc  # noqa: E402


def _results_file(run_dir: str, label: str = "tcp-nomux-w1") -> str:
    return sorted(glob.glob(os.path.join(run_dir, "evidence", label,
                                          "results-*.jsonl")))[0]


def _rewrite(path: str, rows: list) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        for r in rows:
            fh.write(json.dumps(r) + "\n")


def _rows(path: str) -> list:
    return [json.loads(x) for x in open(path, encoding="utf-8") if x.strip()]


class VerdictNegativeCase(unittest.TestCase):
    """任务书点名的负例:每个都必须让判定 FAIL,且原因可追溯。"""

    def test_missing_direction_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            f = _results_file(run)
            keep = [r for r in _rows(f)
                    if not (r.get("metric") == "throughput"
                            and r.get("dir") == "download")]
            _rewrite(f, keep)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["g1_pass"])
            self.assertIsNone(v["download_mbps"])

    def test_low_throughput_is_fail_not_env_invalid(self):
        """有效证据但吞吐低=FAIL;evidence_valid 不得被误伤(非环境无效)。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, tunnel_mbps=2900.0, windows=2)
            v = g1.build_verdict(run, repo)
            self.assertTrue(v["evidence_valid"])
            self.assertFalse(v["g1_pass"])
            self.assertEqual(v["upload_mbps"], 2900)
            self.assertFalse(any("环境受限" in x for x in v["failures"]))

    def test_fewer_than_3_pairs_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, samples=2)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["g1_pass"])
            self.assertFalse(v["evidence_valid"])

    def test_failed_sample_not_silently_filtered(self):
        """invalid 样本 + run-index 仍写 drive_exit=0 → 必须被抓(不可静默过滤)。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            f = _results_file(run)
            rows = _rows(f)
            for r in rows:
                if (r.get("metric") == "throughput" and r.get("path") == "direct"
                        and r.get("streams") == 1 and r.get("dir") == "upload"):
                    r["valid"] = False
                    r["mbps"] = None
                    r["exit"] = 1
                    r["error"] = {"iperf_exit": 1, "stderr_tail": "fx fail"}
                    rid = r["raw_exit"].split("/")[-1]
                    open(os.path.join(run, "evidence", "tcp-nomux-w1", "raw",
                                      rid), "w").write("1\n")
                    break
            _rewrite(f, rows)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])
            self.assertTrue(any("invalid/失败样本" in x for x in v["failures"]))

    def test_source_changed_after_measure_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            with open(os.path.join(repo, "crates", "synthetic", "a.rs"), "a") as fh:
                fh.write("// changed after measure\n")
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])
            self.assertTrue(any("tree_hash 不一致" in x for x in v["failures"]))

    def test_old_binary_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            p = os.path.join(run, "evidence", "tcp-nomux-w1", "endpoints",
                             "client", "endpoint.json")
            doc = json.load(open(p, encoding="utf-8"))
            doc["identity"]["exe_sha256"] = "0" * 64
            json.dump(doc, open(p, "w", encoding="utf-8"))
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])

    def test_wrong_raw_target_fails(self):
        """raw 目标错配(隧道记录实跑直连目标)→ FAIL(单流方向/目标核对)。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            d = os.path.join(run, "evidence", "tcp-nomux-w1")
            victim = next(r for r in _rows(_results_file(run))
                          if r.get("metric") == "throughput"
                          and r.get("path") == "tunnel" and r.get("streams") == 1)
            rawp = os.path.join(d, victim["raw_json"])
            raw = json.load(open(rawp, encoding="utf-8"))
            raw["start"]["connected"][0] = {"remote_host": "10.0.0.2",
                                            "remote_port": 19101}
            json.dump(raw, open(rawp, "w", encoding="utf-8"))
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])

    def test_cpu_coverage_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            p = os.path.join(run, "evidence", "tcp-nomux-w1", "endpoints",
                             "client", "cpu.jsonl")
            lines = open(p, encoding="utf-8").read().splitlines()
            with open(p, "w", encoding="utf-8") as fh:
                fh.write("\n".join(lines[:6]) + "\n")
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["g1_pass"])

    def test_illegal_json_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            f = _results_file(run)
            text = open(f, encoding="utf-8").read().replace(
                '"mbps": 3500', '"mbps": NaN', 1)
            if '"mbps": NaN' not in text:
                text = text.replace('"mbps":3500', '"mbps":NaN', 1)
            with open(f, "w", encoding="utf-8") as fh:
                fh.write(text)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])
            self.assertTrue(any("坏行" in x for x in v["failures"]))

    def test_missing_e2e_with_require_fails_regressions_only(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, e2e=False)
            v = g1.build_verdict(run, repo, require_e2e=True)
            self.assertFalse(v["regressions_pass"])
            self.assertTrue(v["evidence_valid"])   # 缺 E2E ≠ 证据无效
            self.assertTrue(v["g1_pass"])          # G1 判定不受 E2E 缺席影响

    def test_e2e_non_coverage_note_when_absent_and_not_required(self):
        """★独立复核 blocker 1:无 E2E 原件且未设 --require-e2e 时,verdict 必须
        显式声明 regressions_pass 不覆盖 E2E,防止被引用为四套 E2E 证据。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, e2e=False)
            v = g1.build_verdict(run, repo, require_e2e=False)
            self.assertTrue(v["evidence_valid"])
            self.assertTrue(v["g1_pass"])
            self.assertTrue(v["regressions_pass"])
            self.assertTrue(any("[E2E][未覆盖]" in x for x in v["failures"]))
            # 四套原件齐全且未 require → 无注记(证据真实存在且已列路径)
            run2, repo2 = g1._fx_run(td, subdir="full")
            v2 = g1.build_verdict(run2, repo2, require_e2e=False)
            self.assertFalse(any("[E2E][未覆盖]" in x for x in v2["failures"]))

    def test_failed_e2e_originals_fail(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            lg = os.path.join(run, "logs")
            open(os.path.join(lg, "e2e-relay-modes.exit"), "w").write("1\n")
            open(os.path.join(lg, "e2e-relay-modes.log"), "w").write(
                "PASS: a\nFAIL: b\n\n== 结果: PASS=3 FAIL=2 ==\n")
            v = g1.build_verdict(run, repo, require_e2e=True)
            self.assertFalse(v["regressions_pass"])
            self.assertTrue(v["evidence_valid"])

    def test_env_knob_on_acceptance_round_fails(self):
        """最终成绩不得依赖非默认开关:轮记录带 env_extra → FAIL。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            idx = g1.index_load(run)
            idx["rounds"][0]["knobs"] = {"RH_EXPERIMENTAL": "1"}
            g1.index_save(run, idx)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])
            self.assertTrue(any("环境开关" in x for x in v["failures"]))

    def test_drive_failure_recorded_fails(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            idx = g1.index_load(run)
            idx["rounds"][0]["drive_exit"] = 1
            g1.index_save(run, idx)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])
            self.assertTrue(any("drive_exit=1" in x for x in v["failures"]))

    def test_runner_zero_is_not_performance_pass(self):
        """runner(drive)退出码全 0 + 低吞吐 → g1_pass 必须 False。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, tunnel_mbps=2900.0)
            idx = json.load(open(os.path.join(run, "run-index.json"),
                                 encoding="utf-8"))
            self.assertTrue(all(r["drive_exit"] == 0 for r in idx["rounds"]))
            v = g1.build_verdict(run, repo)
            self.assertTrue(v["evidence_valid"])
            self.assertFalse(v["g1_pass"])

    def test_env_limit_verdict_when_direct_download_below_3000(self):
        """fresh direct 下行中位 <3000(≥3对、≥2 独立窗口)→ 环境受限 FAIL 判定。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, tunnel_mbps=2900.0, direct_mbps=2900.0,
                                   windows=2)
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["g1_pass"])
            self.assertTrue(any("环境受限判定·成立" in x for x in v["failures"]))
            # 单窗口不足时只给'待判',不得宣布环境受限
            run1, repo1 = g1._fx_run(td, subdir="one", tunnel_mbps=2900.0,
                                     direct_mbps=2900.0, windows=1)
            v1 = g1.build_verdict(run1, repo1)
            self.assertFalse(any("环境受限判定·成立" in x for x in v1["failures"]))
            self.assertTrue(any("环境受限判定·待判" in x for x in v1["failures"]))

    def test_missing_evidence_dir_for_recorded_round_fails(self):
        """轮记录有、证据目录无(疑似删失败样本)→ FAIL。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, windows=2)
            import shutil
            shutil.rmtree(os.path.join(run, "evidence", "tcp-nomux-w2"))
            v = g1.build_verdict(run, repo)
            self.assertFalse(v["evidence_valid"])
            self.assertTrue(any("证据目录缺失" in x for x in v["failures"]))

    def test_diagnostics_outside_evidence_root_ignored(self):
        """诊断臂在 evidence/ 之外:垃圾目录不影响验收判定。"""
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            d = os.path.join(run, "diagnostics", "g1-tcp-nomux-diag")
            os.makedirs(d)
            with open(os.path.join(d, "junk.log"), "w") as fh:
                fh.write("perturbed garbage\n")
            v = g1.build_verdict(run, repo)
            self.assertTrue(v["evidence_valid"] and v["g1_pass"])


class VerdictPositiveAndSchema(unittest.TestCase):
    def test_good_fixture_passes_with_strict_schema(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, windows=2)
            v = g1.build_verdict(run, repo, require_e2e=True)
            self.assertEqual(sorted(v.keys()), sorted(g1.VERDICT_KEYS))
            self.assertEqual(v["schema"], g1.VERDICT_SCHEMA)
            self.assertTrue(v["evidence_valid"])
            self.assertTrue(v["g1_pass"])
            self.assertTrue(v["regressions_pass"])
            self.assertEqual(v["upload_mbps"], 3500)
            self.assertEqual(v["download_mbps"], 3500)
            self.assertEqual(v["failures"], [])
            self.assertTrue(all(os.path.isabs(p) or p.startswith(run)
                                for p in v["evidence_paths"]))

    def test_cli_stdout_single_strict_json(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td)
            p = subprocess.run(
                [sys.executable, os.path.join(fx.BENCH, "g1_tcp.py"), "verify",
                 "--run-dir", run, "--repo", repo, "--json", "--require-e2e"],
                capture_output=True, text=True, timeout=600)
            self.assertEqual(p.returncode, 0,
                             p.stdout[-2000:] + p.stderr[-2000:])
            lines = [x for x in p.stdout.splitlines() if x.strip()]
            self.assertEqual(len(lines), 1, "stdout 必须只有单个 JSON 对象")

            def _reject(tok):
                raise ValueError(f"非有限数 {tok}")
            doc = json.loads(lines[0], parse_constant=_reject)
            self.assertEqual(sorted(doc.keys()), sorted(g1.VERDICT_KEYS))
            self.assertTrue(doc["g1_pass"])

    def test_cli_fail_exit_nonzero_with_honest_json(self):
        with tempfile.TemporaryDirectory() as td:
            run, repo = g1._fx_run(td, tunnel_mbps=2900.0)
            p = subprocess.run(
                [sys.executable, os.path.join(fx.BENCH, "g1_tcp.py"), "verify",
                 "--run-dir", run, "--repo", repo, "--json"],
                capture_output=True, text=True, timeout=600)
            self.assertEqual(p.returncode, 1)
            doc = json.loads(p.stdout.strip().splitlines()[-1])
            self.assertTrue(doc["evidence_valid"])
            self.assertFalse(doc["g1_pass"])

    def test_gates_verdict_written_once_never_overwritten(self):
        with tempfile.TemporaryDirectory() as td:
            root = os.path.join(td, "proot")
            os.makedirs(os.path.join(root, "gates"))
            run_dir = os.path.join(root, "measure-r0")
            run, repo = g1._fx_run(td, subdir="fixture")
            # 把 fixture 伪装成 measure-r0(仅取其内容:直接改用其路径判定)
            os.rename(run, run_dir)
            buf = io.StringIO()
            with contextlib.redirect_stdout(buf):
                rc = g1.cmd_verify(types.SimpleNamespace(
                    run_dir=run_dir, repo=repo, json=True, require_e2e=False))
            self.assertEqual(rc, 0)
            files = glob.glob(os.path.join(root, "gates", "verdict-*.json"))
            self.assertEqual(len(files), 1)
            doc = json.loads(buf.getvalue())
            self.assertEqual(doc["schema"], g1.VERDICT_SCHEMA)
            # 第二次判定(仍 PASS)→ 新文件(时间戳不同),旧文件不覆盖
            import time as _t
            _t.sleep(1.1)
            with contextlib.redirect_stdout(io.StringIO()):
                g1.cmd_verify(types.SimpleNamespace(
                    run_dir=run_dir, repo=repo, json=True, require_e2e=False))
            files2 = glob.glob(os.path.join(root, "gates", "verdict-*.json"))
            self.assertEqual(len(files2), 2)


class TestPrepare(unittest.TestCase):
    """prepare:只创建不存在目录;基线与历史证据永不覆盖;快照=当前未提交树。"""

    def _prepare(self, tmp, name="root"):
        root = os.path.join(tmp, name)
        return g1.cmd_prepare(types.SimpleNamespace(out_dir=root)), root

    def test_prepare_creates_all_artifacts(self):
        with tempfile.TemporaryDirectory() as td:
            rc, root = self._prepare(td)
            self.assertEqual(rc, 0)
            for f in ("HEAD.txt", "git-status.txt", "git-status.sha256",
                      "baseline-modifications.json", "prepared.json",
                      "example-contract.md"):
                self.assertTrue(os.path.isfile(os.path.join(root, f)), f)
            self.assertTrue(os.path.isdir(os.path.join(root, "gates")))
            self.assertTrue(os.path.isdir(os.path.join(root, "baseline-source")))
            self.assertTrue(os.path.isfile(os.path.join(
                root, "baseline-source", "Cargo.toml")))
            doc = json.load(open(os.path.join(root, "prepared.json"),
                                 encoding="utf-8"))
            self.assertTrue(g1.HEX64.match(doc["baseline_source"]["tree_hash"]))
            self.assertTrue(g1.HEX64.match(doc["g1_tcp_sha256"]))
            self.assertTrue(g1.HEX64.match(doc["test_g1_tcp_sha256"]))
            # HEAD 与当前一致;修改基线覆盖 git status 全部条目
            head = open(os.path.join(root, "HEAD.txt")).read().strip()
            self.assertEqual(head, cc.run_local(
                ["git", "-C", g1.REPO_ROOT, "rev-parse", "HEAD"], 60)[1]
                .decode().strip())
            status = open(os.path.join(root, "git-status.txt")).read()
            entries = json.load(open(os.path.join(root,
                                                  "baseline-modifications.json"),
                                     encoding="utf-8"))["entries"]
            for line in status.splitlines():
                if line.strip():
                    self.assertIn(line[3:], entries)

    def test_prepare_refuses_second_run(self):
        with tempfile.TemporaryDirectory() as td:
            self._prepare(td)
            with self.assertRaises(SystemExit):
                self._prepare(td)

    def test_prepare_refuses_foreign_content(self):
        with tempfile.TemporaryDirectory() as td:
            root = os.path.join(td, "root")
            os.makedirs(os.path.join(root, "old-evidence"))
            with self.assertRaises(SystemExit):
                g1.cmd_prepare(types.SimpleNamespace(out_dir=root))

    def test_prepare_accepts_identical_contract_only(self):
        with tempfile.TemporaryDirectory() as td:
            root = os.path.join(td, "root")
            os.makedirs(root)
            with open(os.path.join(root, "example-contract.md"), "w",
                      encoding="utf-8") as fh:
                fh.write(g1.EXAMPLE_CONTRACT)
            self.assertEqual(g1.cmd_prepare(
                types.SimpleNamespace(out_dir=root)), 0)
            # 篡改契约的目录 → 拒绝
            root2 = os.path.join(td, "root2")
            os.makedirs(root2)
            with open(os.path.join(root2, "example-contract.md"), "w",
                      encoding="utf-8") as fh:
                fh.write("# tampered\n")
            with self.assertRaises(SystemExit):
                g1.cmd_prepare(types.SimpleNamespace(out_dir=root2))

    def test_snapshot_is_uncommitted_tree_not_head(self):
        with tempfile.TemporaryDirectory() as td:
            _rc, root = self._prepare(td)
            status = open(os.path.join(root, "git-status.txt"),
                          encoding="utf-8").read()
            mod = next((ln[3:] for ln in status.splitlines()
                        if ln.startswith(" M ")), None)
            self.assertIsNotNone(mod, "当前工作区应有已修改文件")
            snap = open(os.path.join(root, "baseline-source", mod), "rb").read()
            live = open(os.path.join(g1.REPO_ROOT, mod), "rb").read()
            _rc, head_out, _e = cc.run_local(
                ["git", "-C", g1.REPO_ROOT, "show", f"HEAD:{mod}"], 60)
            self.assertEqual(snap, live)        # 快照=当前工作区内容
            self.assertNotEqual(snap, head_out)  # 且 ≠ HEAD(不是 git archive)


class TestFixedAb(unittest.TestCase):
    def test_summarize_small_packet_full_range(self):
        """★独立复核 blocker 2:汇总必须给小包全量口径(median/min/max/n+逐轮),
        防止文档只引单轮值(如 r2)冒充全量。"""
        results = [
            {"round": 1, "arm": t, "direction": d,
             "gbps": 5.0 + 0.1 * i,
             "small_packet": {"p50_us": p50, "p99_us": p50 + 70}}
            for i, (r, t, d, p50) in enumerate([
                (1, "old", "upload", 105.3), (2, "old", "upload", 121.4),
                (3, "old", "upload", 96.3),
                (1, "new", "upload", 106.3), (2, "new", "upload", 97.9),
                (3, "new", "upload", 89.7),
                (1, "old", "download", 113.4), (2, "old", "download", 114.1),
                (3, "old", "download", 90.8),
                (1, "new", "download", 131.4), (2, "new", "download", 95.4),
                (3, "new", "download", 90.2)])
        ]
        s = g1.fixed_ab_summarize(results)
        dl_new = s["download-new"]["small_packet_p50_us"]
        self.assertEqual(dl_new["n"], 3)
        self.assertEqual(dl_new["median"], 95.4)
        self.assertEqual(dl_new["min"], 90.2)
        self.assertEqual(dl_new["max"], 131.4)   # 全表最差单值必须可见
        up_old = s["upload-old"]["small_packet_p50_us"]
        self.assertEqual(up_old["median"], 105.3)
        self.assertEqual(up_old["min"], 96.3)
        self.assertEqual(up_old["max"], 121.4)
        self.assertEqual(len(s["download-old"]["per_round"]), 3)
        # 小包缺失时聚合为 None(不伪造)
        s2 = g1.fixed_ab_summarize([{"round": 1, "arm": "old", "direction":
                                     "upload", "gbps": 5.0}])
        self.assertIsNone(s2["upload-old"]["small_packet_p50_us"])

    def test_tick_command_wrapped_in_subshell_for_redirection(self):
        """★round-4 诊断事故:分号列表直接接 `>> log` 只重定向最后一条命令
        (实测仅落 pipe_max 一行/tick)。必须子 shell 包裹整体重定向;
        pipe 探针失败要有显式回退。"""
        with tempfile.TemporaryDirectory() as td:
            r = g1.G1RoundRunner(_StubTm(), cc.Progress(td), td, "/tmp/rh-fx",
                                 cc.RoundCfg("tcp", 0, "fx", "fx"))
            r.roles = [(None, "server/server.pid", 4242, "server.pid")]
            cmd = r._tick_append_cmd("server", ("server.pid",),
                                     "/tmp/rh-fx/diag.log")
            self.assertTrue(cmd.startswith("( "))
            self.assertTrue(cmd.endswith(") >> /tmp/rh-fx/diag.log 2>&1"))
            self.assertIn("ss -tinm", cmd)
            self.assertIn("|| echo pipe_sz_default=unavailable", cmd)

    def test_salvage_extracts_termark_output_and_refuses_overwrite(self):
        """★round-4 诊断恢复:termark 日志首行命令含 awk 花括号,不能从头
        find('{')(会命中命令行);从整行 `{` 起解析。恢复只增不覆盖。"""
        with tempfile.TemporaryDirectory() as td:
            run = os.path.join(td, "run")
            logs = os.path.join(run, "logs")
            os.makedirs(logs)
            os.makedirs(os.path.join(run, "diagnostics", "diag-label"))
            tick_out = "===== tick ... =====\nESTAB ... cwnd:10 app_limited"
            termark_log = (
                "$ termark exec fx 'echo x; awk '\"'\"'{print $1}'\"'\"' f'\n"
                "{\n  \"output\": " + json.dumps(tick_out) + ",\n"
                "  \"exitCode\": 0,\n  \"durationMs\": 5\n}\n")
            open(os.path.join(logs, "0007-diag-tick-server-3.log"),
                 "w", encoding="utf-8").write(termark_log)
            open(os.path.join(run, "run-index.json"), "w",
                 encoding="utf-8").write(json.dumps(
                     {"schema": g1.SCHEMA, "diagnostics_rounds":
                      [{"label": "diag-label"}]}))
            rc = g1.cmd_internal_salvage_diag(types.SimpleNamespace(
                run_dir=run, label=None))
            self.assertEqual(rc, 0)
            got = open(os.path.join(run, "diagnostics", "diag-label",
                                    "server-diag-ss-proc-recovered.log"),
                       encoding="utf-8").read()
            self.assertIn("cwnd:10", got)
            self.assertIn("logs/0007-diag-tick-server-3.log", got)
            with self.assertRaises(SystemExit):     # 只增不覆盖
                g1.cmd_internal_salvage_diag(types.SimpleNamespace(
                    run_dir=run, label=None))

    def test_no_example_returns_nonzero_unavailable(self):
        with tempfile.TemporaryDirectory() as td:
            fake = os.path.join(td, "cand-no-example")
            os.makedirs(fake)
            out = os.path.join(td, "out")
            rc = g1.cmd_fixed_ab(types.SimpleNamespace(
                out_dir=out, source=fake, baseline=None, prepare_root=td,
                mib=g1.FIXED_AB_MIB, rounds=1))
            self.assertEqual(rc, 3)
            self.assertFalse(os.path.exists(out))   # 不可用不建目录不冒充

    def test_missing_baseline_source_fails(self):
        with tempfile.TemporaryDirectory() as td:
            fake = os.path.join(td, "cand")
            exd = os.path.join(fake, *g1.EXAMPLE_REL.split("/"))
            os.makedirs(os.path.dirname(exd), exist_ok=True)
            open(exd, "w").write("fn main(){}\n")
            with self.assertRaises(SystemExit):
                g1.cmd_fixed_ab(types.SimpleNamespace(
                    out_dir=os.path.join(td, "out"), source=fake,
                    baseline=None, prepare_root=td, mib=8, rounds=1))

    def test_baseline_overlay_same_example(self):
        """基线树叠加候选 example 原样副本:同计数同钩子,唯一差异是泵。"""
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
            open(os.path.join(base, "crates", "x", "src", "lib.rs"), "a").write(
                "// old pump\n")
            open(os.path.join(cand, "crates", "x", "src", "lib.rs"), "a").write(
                "// new pump\n")
            exd = os.path.join(cand, *g1.EXAMPLE_REL.split("/"))
            os.makedirs(os.path.dirname(exd), exist_ok=True)
            open(exd, "w").write("fn main(){}\n")
            info = g1.fixed_ab_prepare_trees(td, cand, base)
            ex_base = os.path.join(info["baseline"], *g1.EXAMPLE_REL.split("/"))
            self.assertTrue(os.path.isfile(ex_base))
            self.assertEqual(g1.sha256_file(ex_base), info["example_sha256"])
            self.assertNotEqual(info["candidate_tree_hash"],
                                info["baseline_tree_hash"])

    def test_refuses_existing_out_dir(self):
        with tempfile.TemporaryDirectory() as td:
            fake = os.path.join(td, "cand")
            exd = os.path.join(fake, *g1.EXAMPLE_REL.split("/"))
            os.makedirs(os.path.dirname(exd), exist_ok=True)
            open(exd, "w").write("fn main(){}\n")
            out = os.path.join(td, "out")
            os.makedirs(out)
            open(os.path.join(out, "keep"), "w").write("x")
            with self.assertRaises(SystemExit):
                g1.cmd_fixed_ab(types.SimpleNamespace(
                    out_dir=out, source=fake, baseline=td, prepare_root=td,
                    mib=8, rounds=1))
            self.assertTrue(os.path.isfile(os.path.join(out, "keep")))


class TestToolShaDisclosure(unittest.TestCase):
    def test_tool_shas_recorded(self):
        shas = g1.tool_shas()
        self.assertTrue(g1.HEX64.match(shas["g1_tcp_sha256"]))
        self.assertTrue(g1.HEX64.match(shas["test_g1_tcp_sha256"]))
        self.assertEqual(shas["g1_tcp_sha256"],
                         g1.sha256_file(os.path.join(fx.BENCH, "g1_tcp.py")))


class _StubTm:
    """RoundRunner 构造不触远端;最小 tm 假体。"""

    progress = None


class TestRoundRunnerInheritance(unittest.TestCase):
    """★事故回归(2026-10-15 baseline 首跑 exit 1):G1RoundRunner 实例属性
    `diagnostics`(bool)曾遮蔽继承的 closure RoundRunner.diagnostics() 方法,
    run() 内 self.diagnostics() → 'bool' object is not callable,
    collect() 不执行、证据丢失。锁定:继承方法全部保持可调用。"""

    def test_closure_methods_not_shadowed(self):
        with tempfile.TemporaryDirectory() as td:
            prog = cc.Progress(td)
            cfg = cc.RoundCfg("tcp", 0, "fx-label", "fx-run")
            r = g1.G1RoundRunner(_StubTm(), prog, td, "/tmp/rh-fx", cfg,
                                 evidence_root=os.path.join(td, "diag"),
                                 diag_mode=True)
            for name in ("diagnostics", "collect", "run", "stop_all",
                         "start_server", "start_client", "run_drive",
                         "apply_affinity", "_record_role"):
                self.assertTrue(callable(getattr(r, name, None)),
                                f"{name} 被实例属性遮蔽(继承方法必须可调用)")
            self.assertIsInstance(r.diag_mode, bool)
            self.assertEqual(r.evidence_root, os.path.join(td, "diag"))
            # 默认构造(验收轮):evidence_root=out_dir/evidence,diag_mode=False
            r2 = g1.G1RoundRunner(_StubTm(), prog, td, "/tmp/rh-fx", cfg)
            self.assertEqual(r2.evidence_root, os.path.join(td, "evidence"))
            self.assertFalse(r2.diag_mode)
            self.assertTrue(callable(r2.diagnostics))


class TestSelfTestCLI(unittest.TestCase):
    def test_self_test_exit_zero(self):
        p = subprocess.run([sys.executable, os.path.join(fx.BENCH, "g1_tcp.py"),
                            "--self-test"], capture_output=True, text=True,
                           timeout=900)
        self.assertEqual(p.returncode, 0, p.stdout[-3000:] + p.stderr[-2000:])


if __name__ == "__main__":
    unittest.main()
