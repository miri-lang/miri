# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/diagnose.py: reading a sampled profile, folding it,
ranking its symbols, drawing it, and choosing the cells to diagnose."""

import sys
import unittest
import xml.dom.minidom
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "bench"))

import diagnose  # noqa: E402

# The shape of macOS `sample` output: a call graph whose depth is its
# indentation, each line a sample count and a frame.
PROFILE = """Analysis of sampling nb (pid 1) every 1 millisecond
Call graph:
    10 Thread_1: Main Thread
    + 10 _dyld_start  (in dyld) + 0  [0x1]
    20 Thread_1   DispatchQueue_1: com.apple.main-thread  (serial)
      20 start  (in dyld) + 6992  [0x2]
      ! 20 main  (in nb) + 644  [0x3]
      !   15 miri.kernel  (in nb) + 1540  [0x4]
      !   : 9 miri.advance  (in nb) + 4080,4048,...  [0x5,0x6]
      !   : 4 miri_rt_rc_is_shared  (in nb) + 84  [0x7]
      !   5 miri.read_input  (in nb) + 12  [0x8]

Total number in stack (recursive counted multiple times):
"""


class ProfileTest(unittest.TestCase):
    def setUp(self):
        self.folded = diagnose.folded_stacks(diagnose.call_tree(PROFILE))

    def test_every_sample_lands_in_exactly_one_stack(self):
        self.assertEqual(sum(self.folded.values()), 30)

    def test_a_frame_keeps_only_the_samples_its_callees_do_not_take(self):
        self.assertEqual(self.folded["Thread_1   DispatchQueue_1: com.apple.main-thread  (serial);start;main;miri.kernel"], 2)
        self.assertEqual(self.folded["Thread_1   DispatchQueue_1: com.apple.main-thread  (serial);start;main;miri.kernel;miri.advance"], 9)

    def test_an_unwalkable_stack_is_named_as_such(self):
        self.assertEqual(self.folded["Thread_1: Main Thread;[stack not walkable]"], 10)

    def test_top_symbols_rank_by_self_samples(self):
        top = diagnose.top_symbols(self.folded)
        self.assertEqual([t["symbol"] for t in top[:3]], ["[stack not walkable]", "miri.advance", "miri.read_input"])
        self.assertAlmostEqual(sum(t["share"] for t in top), 1.0)
        self.assertEqual(diagnose.hot_miri_functions(top), ["miri.advance", "miri.read_input", "miri.kernel"])

    def test_a_function_of_an_imported_module_is_a_miri_function(self):
        names = ["miri$system$json.JsonWriter.write_string", "miri_rt_rc_is_shared", "miri.advance", "memmove"]
        top = [{"symbol": name, "share": 0.25} for name in names]
        self.assertEqual(diagnose.hot_miri_functions(top), ["miri$system$json.JsonWriter.write_string", "miri.advance"])

    def test_the_flamegraph_is_well_formed_svg(self):
        svg = diagnose.flamegraph_svg(self.folded, "nbody s miri")
        document = xml.dom.minidom.parseString(svg)
        self.assertEqual(document.documentElement.tagName, "svg")
        self.assertIn("miri.advance", svg)


class ListingTest(unittest.TestCase):
    def test_a_listing_splits_into_its_functions(self):
        text = "; miri.a\nfunction %a() {\n}\n; miri.b\nfunction %b() {\n}\n"
        sections = diagnose.listing_sections(text, "; ")
        self.assertEqual(list(sections), ["miri.a", "miri.b"])
        self.assertIn("function %b", sections["miri.b"])


class SelectionTest(unittest.TestCase):
    def test_only_miri_cells_marked_for_diagnosis_are_chosen(self):
        summary = {
            "cells": [
                {"workload": "nbody", "language": "miri", "size": "s", "mode": "cold", "diagnose": True},
                {"workload": "nbody", "language": "miri", "size": "s", "mode": "hot", "diagnose": True},
                {"workload": "sieve", "language": "miri", "size": "s", "mode": "cold", "diagnose": False},
                {"workload": "sieve", "language": "c", "size": "s", "mode": "cold", "diagnose": True},
            ]
        }
        self.assertEqual(diagnose.lost_cells(summary, None), [("nbody", "s")])
        self.assertEqual(diagnose.lost_cells(summary, "sieve/m"), [("sieve", "m")])

    def test_perf_output_becomes_values_and_ipc(self):
        values = diagnose.perf_values("2000,,cycles,1,100.00,,\n3000,,instructions,1,100.00,,\n")
        self.assertEqual(values["cycles"], 2000)
        self.assertAlmostEqual(values["ipc"], 1.5)


if __name__ == "__main__":
    unittest.main()
