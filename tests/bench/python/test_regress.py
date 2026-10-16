# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/regress.py on synthetic nights: a planted slowdown is
flagged on its second night and not its first, an unchanged compiler is never
flagged, and a recovery closes what a regression opened."""

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "bench"))

import regress  # noqa: E402
import run  # noqa: E402

CHARTER = run.load_charter()


def night(run_id, miri_ratio, cv=0.01):
    """A summary whose Miri nbody cell takes `miri_ratio` of C's time."""
    return {
        "run_id": run_id,
        "cells": [
            {"workload": "nbody", "language": "c", "size": "s", "mode": "cold", "vs_c": {"time": {"value": 1.0, "cv": cv}}},
            {"workload": "nbody", "language": "miri", "size": "s", "mode": "cold", "vs_c": {"time": {"value": miri_ratio, "cv": cv}}},
        ],
    }


def history(ratios):
    """Earlier nights, each judged against the ones before it, as the nightly
    job would have."""
    nights = []
    for i, ratio in enumerate(ratios):
        summary = night(f"n{i:02d}", ratio)
        nights.append((summary, regress.judge(summary, nights, CHARTER)))
    return nights


class RegressTest(unittest.TestCase):
    def test_seven_unchanged_nights_flag_nothing(self):
        nights = history([2.0, 2.01, 1.99, 2.0, 2.02, 1.98, 2.0])
        for _, verdicts in nights:
            self.assertEqual(verdicts["regressed"], [])
            self.assertEqual(verdicts["suspect"], [])

    def test_a_planted_slowdown_is_suspect_first_and_a_regression_second(self):
        nights = history([2.0] * 7)
        first = regress.judge(night("slow1", 2.3), nights, CHARTER)
        self.assertEqual([s["cell"] for s in first["suspect"]], ["nbody/miri/s/cold"])
        self.assertEqual(first["regressed"], [])
        nights.append((night("slow1", 2.3), first))
        second = regress.judge(night("slow2", 2.3), nights, CHARTER)
        self.assertEqual([s["cell"] for s in second["regressed"]], ["nbody/miri/s/cold"])
        self.assertAlmostEqual(second["regressed"][0]["change"], 2.3 / 2.0, places=2)

    def test_a_slowdown_inside_the_noise_is_not_suspect(self):
        nights = history([2.0] * 7)
        verdicts = regress.judge(night("noisy", 2.3, cv=0.08), nights, CHARTER)
        self.assertEqual(verdicts["suspect"], [])

    def test_a_recovery_is_reported_once_the_cell_returns(self):
        nights = history([2.0] * 7 + [2.3, 2.3])
        self.assertTrue(nights[-1][1]["regressed"])
        verdicts = regress.judge(night("back", 2.0), nights, CHARTER)
        self.assertEqual(verdicts["recovered"], [{"cell": "nbody/miri/s/cold", "measure": "time_vs_c"}])

    def test_the_issue_title_names_the_workload_measure_and_change(self):
        title = regress.issue_title({"cell": "nbody/miri/s/cold", "measure": "time_vs_c", "change": 1.15}, "0123456789abcdef")
        self.assertEqual(title, "perf: nbody time_vs_c regressed 15% since 0123456789")


if __name__ == "__main__":
    unittest.main()
