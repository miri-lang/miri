# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/diagnose.py: reading a sampled profile, folding it,
ranking its symbols, drawing it, choosing the cells to diagnose, and which
build of each port each step runs."""

import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest import mock
import xml.dom.minidom
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "bench"))

import diagnose  # noqa: E402
import run  # noqa: E402

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


class BuildChoiceTest(unittest.TestCase):
    """The sampled profile of the Miri port comes from its `--profile` build,
    which keeps frame pointers; the counters and timing stay on the release
    build the published numbers come from."""

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        work = Path(self.scratch.name)
        self.ctx = SimpleNamespace(
            work_dir=work,
            miri=Path("miri"),
            charter={"languages": {"miri": {"build": "miri build --release --opt-level 3 -o {out} {src}"}}},
            build_env=lambda language: {},
        )
        self.workload = SimpleNamespace(name="nbody", manifest={}, port=lambda language: work / f"nbody.{language}", blocked=lambda language: False)
        for language in diagnose.LANGUAGES:
            self.workload.port(language).write_text("")

    def release(self, ctx, workload_name, language, src, variant):
        return run.Built(language, [f"{language}-release"], None)

    def built_ports(self):
        lines = []

        def profile(ctx, language, line, src, out):
            lines.append(line)
            return run.Built(language, [f"{language}-profile"], None)

        with mock.patch.object(run, "build", self.release), mock.patch.object(run, "build_with_line", profile), mock.patch.object(diagnose.subprocess, "run"):
            ports = diagnose.build_ports(self.ctx, self.workload, self.ctx.work_dir)
        return ports, lines

    def test_the_miri_port_is_sampled_from_its_profile_build(self):
        ports, lines = self.built_ports()
        self.assertEqual(lines, ["miri build --profile --opt-level 3 -o {out} {src}"])
        self.assertEqual(ports.sampled["miri"].run_words, ["miri-profile"])
        self.assertEqual(ports.timed["miri"].run_words, ["miri-release"])

    def test_the_other_ports_are_sampled_from_their_one_build(self):
        ports, _ = self.built_ports()
        for language in ("c", "rust"):
            self.assertIs(ports.sampled[language], ports.timed[language])

    def test_a_failed_profile_build_is_named_in_place_of_the_profile(self):
        with mock.patch.object(run, "build", self.release), mock.patch.object(run, "build_with_line", return_value="build-failed"), mock.patch.object(diagnose.subprocess, "run"):
            ports = diagnose.build_ports(self.ctx, self.workload, self.ctx.work_dir)
        self.assertEqual(ports.sampled["miri"], "build-failed")
        self.assertEqual(ports.timed["miri"].run_words, ["miri-release"])

    def test_counters_run_the_release_build_and_sampling_the_profile_build(self):
        ports, _ = self.built_ports()
        sampled = []
        out_dir = self.ctx.work_dir / "out"
        summary = {"cells": []}
        with (
            mock.patch.object(diagnose, "build_ports", return_value=ports),
            mock.patch.object(diagnose, "counters", return_value={}) as counted,
            mock.patch.object(diagnose, "sample_profile", side_effect=lambda port, *rest: sampled.append(port.run_words[0])),
            mock.patch.object(diagnose, "runtime_counts", return_value={}),
            mock.patch.object(diagnose, "copy_listings"),
            mock.patch.object(run, "input_file", return_value=Path("input")),
        ):
            diagnose.diagnose_cell(self.ctx, summary, self.workload, "s", out_dir)
        self.assertEqual({language: port.run_words[0] for language, port in counted.call_args.args[0].items()}, {"miri": "miri-release", "c": "c-release", "rust": "rust-release"})
        self.assertEqual(sampled, ["miri-profile", "c-release", "rust-release"])

    def test_a_failed_profile_build_is_reported_in_the_bundle(self):
        timed = {"miri": run.Built("miri", ["miri-release"], None)}
        ports = diagnose.Ports(timed, {"miri": "build-failed"}, self.ctx.work_dir / "listing")
        out_dir = self.ctx.work_dir / "out"
        with (
            mock.patch.object(diagnose, "build_ports", return_value=ports),
            mock.patch.object(diagnose, "counters", return_value={}),
            mock.patch.object(diagnose, "sample_profile") as sampler,
            mock.patch.object(diagnose, "runtime_counts", return_value={}),
            mock.patch.object(diagnose, "copy_listings"),
            mock.patch.object(run, "input_file", return_value=Path("input")),
        ):
            diagnose.diagnose_cell(self.ctx, {"cells": []}, self.workload, "s", out_dir)
        sampler.assert_not_called()
        self.assertEqual((out_dir / "profile.miri.txt").read_text(), "unavailable: the profile build of the port: build-failed\n")


if __name__ == "__main__":
    unittest.main()
