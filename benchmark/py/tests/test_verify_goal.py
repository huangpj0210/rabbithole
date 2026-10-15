"""verify_goal 回归:证据 schema 核验 fail closed。

--self-test 的完整对抗矩阵由 CLI 运行并留档(见 harness/logs/);
此处锁定同模块的两个关键面:完整好证据 PASS、单点破坏 FAIL。
"""
from __future__ import annotations

import json
import os
import sys
import unittest

sys.path.insert(0, __file__.rsplit("/", 1)[0])
import fixtures as fx  # noqa: E402
import verify_goal  # noqa: E402


class TestVerifyGoal(unittest.TestCase):
    def test_good_set_passes(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            verify_goal.build_good_set(tmp)
            ok, report = verify_goal.verify_path(tmp)
            self.assertTrue(ok, report)
            self.assertIn("总体:PASS", report)

    def test_missing_schema_fails(self):
        import tempfile
        with tempfile.TemporaryDirectory() as tmp:
            verify_goal.build_good_set(tmp)
            os.remove(os.path.join(tmp, "tcp-nomux", "schema.json"))
            ok, report = verify_goal.verify_path(tmp)
            self.assertFalse(ok)
            self.assertIn("schema", report)

    def test_thresholds_not_loosened(self):
        """阈值锁:修测量方法不得改变 threshold(docs §1 原表)。"""
        self.assertEqual(verify_goal.THRESH["g1_mbps"], 3000.0)
        self.assertEqual(verify_goal.THRESH["g1_cpu_pct"], 50.0)
        self.assertEqual(verify_goal.THRESH["g2_mbps"], 2500.0)
        self.assertEqual(verify_goal.THRESH["g4_loss_pct"], 1.0)
        self.assertEqual(verify_goal.THRESH["g5_connections"], 4096)
        self.assertEqual(verify_goal.THRESH["g6_retention"], 0.80)
        self.assertEqual(verify_goal.THRESH["g7_min_pairs"], 3)
        self.assertEqual(verify_goal.PROTOS, ("tcp", "tls", "ws", "wss", "quic", "kcp"))
        self.assertEqual(verify_goal.STREAM_TIERS, (1, 4, 16))

    def test_cpu_coverage_and_pct_math(self):
        """§1.1 口径:覆盖率 = 窗口样本数/(窗口秒×hz);CPU% = Δticks/CLK_TCK/窗口秒×100。"""
        header = {"clk_tck": 100, "hz": 20, "pid": 1}
        samples = [{"t_us": 1_000_000, "ticks": 100, "rss_kb": 1},
                   {"t_us": 1_050_000, "ticks": 103, "rss_kb": 1},
                   {"t_us": 1_100_000, "ticks": 106, "rss_kb": 1},
                   {"t_us": 1_150_000, "ticks": 109, "rss_kb": 1},
                   {"t_us": 1_200_000, "ticks": 112, "rss_kb": 1},
                   {"t_us": 1_900_000, "ticks": 999, "rss_kb": 1}]
        res = verify_goal.cpu_window(header, samples, 1_000_000, 1_200_000)
        # 0.2s 窗口 × 20Hz = 4 期望;窗口内 5 样本 → 覆盖率封顶 1.0
        self.assertEqual(res["coverage"], 1.0)
        # Δticks=12, Δt=0.2s, CLK_TCK=100 → 12/100/0.2×100 = 60%
        self.assertEqual(res["cpu_pct"], 60.0)
        # 窗口内只有 1 个样本 → 覆盖率骤降且算不出 CPU
        res2 = verify_goal.cpu_window(header, samples, 1_500_000, 1_600_000)
        self.assertEqual(res2["coverage"], 0.0)
        self.assertIsNone(res2["cpu_pct"])

    def test_hand_filled_fields_defined_ignored(self):
        """手填 pass 字段必须在忽略清单里(核验只认原始证据)。"""
        self.assertIn("pass", verify_goal.HAND_FILLED_FIELDS)
        self.assertIn("verdict", verify_goal.HAND_FILLED_FIELDS)

    def test_median(self):
        self.assertEqual(verify_goal.median([3, 1, 2]), 2)
        self.assertEqual(verify_goal.median([1, 2, 3, 4]), 2.5)
        self.assertIsNone(verify_goal.median([]))


if __name__ == "__main__":
    unittest.main()
