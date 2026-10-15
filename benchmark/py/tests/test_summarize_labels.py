"""A10 回归:分实验(label 键)汇总 + 中位数 + 失败显式(红→绿)。

红:修复前 summarize 的吞吐键只有 proto,同键后写覆盖先写 ——
    peer-f2 目录 8 个 jsonl 同为 tcp 时只有末文件的值存活。
绿:现行 summarize_peer 键含 label,同键多样本给中位数/最小/最大,
    无效样本显式列出,不造 0。
"""
from __future__ import annotations

import json
import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, __file__.rsplit("/", 1)[0])
import fixtures as fx  # noqa: E402
import summarize as new_summarize  # noqa: E402

old_summarize = fx.load_before_fix("summarize.py")


def old_fmt_records(label, tunnel_mbps):
    """旧版记录格式(无 valid/label 语义)。"""
    return [
        {"metric": "throughput", "path": "direct", "dir": "upload", "streams": 1,
         "mbps": 5000, "proto": "tcp"},
        {"metric": "throughput", "path": "tunnel", "dir": "upload", "streams": 1,
         "mbps": tunnel_mbps, "proto": "tcp"},
    ]


class TestLabelKeys(unittest.TestCase):
    def test_red_old_summarize_collapses_labels(self):
        """红证:两个实验(label a=1000 / b=2000)在旧汇总里只剩末者。"""
        rd = Path(fx.tmp_dir())
        fa = Path(fx.write_results(str(rd), "results-a.jsonl", old_fmt_records("a", 1000)))
        fb = Path(fx.write_results(str(rd), "results-b.jsonl", old_fmt_records("b", 2000)))
        outdir = Path(fx.tmp_dir())
        old_summarize.summarize_peer(rd, [fa, fb], outdir)
        md = (outdir / "summary.md").read_text(encoding="utf-8")
        rows = [ln for ln in md.splitlines()
                if ln.startswith("| tcp |") and "| upload | 1 |" in ln]
        self.assertEqual(len(rows), 1, "旧汇总每个 (proto,dir,streams) 只有一行")
        self.assertIn("| 5000 | 2000 |", rows[0])
        self.assertNotIn("1000", rows[0], "label a 的隧道值被后写覆盖(A10 缺陷)")

    def test_new_summarize_keeps_both_labels(self):
        rd = Path(fx.tmp_dir())
        recs = []
        for label, mbps in (("a", 1000), ("b", 2000)):
            recs.append(fx.thp(label, "direct", "upload", 1, 1, 5000))
            recs.append(fx.thp(label, "tunnel", "upload", 1, 1, mbps))
        fx.write_results(str(rd), "results-a.jsonl", recs[:2])
        fx.write_results(str(rd), "results-b.jsonl", recs[2:])
        outdir = Path(fx.tmp_dir())
        new_summarize.summarize_peer(rd, sorted(rd.glob("results-*.jsonl")), outdir)
        md = (outdir / "summary.md").read_text(encoding="utf-8")
        rows = [ln for ln in md.splitlines()
                if ln.startswith("| a |") or ln.startswith("| b |")]
        self.assertEqual(len(rows), 2, md)
        joined = "\n".join(rows)
        self.assertIn("1000", joined)
        self.assertIn("2000", joined)

    def test_median_of_three_samples(self):
        rd = Path(fx.tmp_dir())
        recs = []
        for sample, mbps in ((1, 100), (2, 200), (3, 300)):
            recs.append(fx.thp("m", "direct", "upload", 1, sample, 1000))
            recs.append(fx.thp("m", "tunnel", "upload", 1, sample, mbps))
        fx.write_results(str(rd), "results-m.jsonl", recs)
        outdir = Path(fx.tmp_dir())
        new_summarize.summarize_peer(rd, sorted(rd.glob("results-*.jsonl")), outdir)
        md = (outdir / "summary.md").read_text(encoding="utf-8")
        self.assertIn("200 / 100 / 300 (n=3)", md)

    def test_invalid_samples_listed_not_zeroed(self):
        """无效样本显式出现在"无效样本"节,不冒充 0。"""
        rd = Path(fx.tmp_dir())
        recs = [
            fx.thp("v", "direct", "upload", 1, 1, 5000),
            fx.thp("v", "tunnel", "upload", 1, 1, None, valid=False),
        ]
        fx.write_results(str(rd), "results-v.jsonl", recs)
        outdir = Path(fx.tmp_dir())
        new_summarize.summarize_peer(rd, sorted(rd.glob("results-*.jsonl")), outdir)
        md = (outdir / "summary.md").read_text(encoding="utf-8")
        self.assertIn("无效样本 / 失败(1 条)", md)
        self.assertIn("throughput_invalid", md)
        # 唯一 tunnel 样本无效 → 吞吐表不出现该实验的行(无配对可算),
        # 且任何单元格都不含 "0"(无效不得折算成 0)
        self.assertFalse([ln for ln in md.splitlines() if ln.startswith("| v |")],
                         "无效 tunnel 样本不得产生吞吐行")
        payload = json.loads((outdir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(payload["invalid"][0]["kind"], "throughput_invalid")

    def test_bad_json_lines_reported(self):
        rd = Path(fx.tmp_dir())
        p = os.path.join(rd, "results-bad.jsonl")
        with open(p, "w", encoding="utf-8") as fh:
            fh.write('{"metric":"throughput","mbps":,\n')   # 旧版会"修复"这种坏行
            fh.write(json.dumps(fx.thp("bad", "direct", "upload", 1, 1, 100)) + "\n")
        outdir = Path(fx.tmp_dir())
        new_summarize.summarize_peer(Path(rd), [Path(p)], outdir)
        payload = json.loads((outdir / "summary.json").read_text(encoding="utf-8"))
        self.assertEqual(len(payload["bad_lines"]), 1, payload["bad_lines"])
        self.assertEqual(payload["bad_lines"][0]["line"], 1)


if __name__ == "__main__":
    unittest.main()
