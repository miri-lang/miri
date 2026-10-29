# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/diagnose.py: reading a sampled profile from macOS
`sample` or Linux `perf`, folding it, ranking its symbols, drawing it,
choosing the cells to diagnose, which build of each port each step runs, and
how the bundle names those builds."""

import os
import subprocess
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
            mock.patch.object(diagnose, "sample_profile", side_effect=lambda port, *rest: sampled.append(port.run_words[0]) or "no profiler"),
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


FIXTURES = Path(__file__).resolve().parent / "fixtures"
PERF_ROOT = "binary-trees.mi;_start;__libc_start_main;[libc.so.6];main"


class PerfFoldTest(unittest.TestCase):
    """`perf script` output of the Miri port of binary-trees, recorded on
    Linux, folds into the same stacks the macOS profile does."""

    def setUp(self):
        self.folded = diagnose.folded_perf_stacks((FIXTURES / "perf_script.binary-trees.txt").read_text())

    def test_every_sample_lands_in_exactly_one_stack(self):
        self.assertEqual(sum(self.folded.values()), 10)

    def test_a_stack_runs_from_the_thread_to_the_sampled_frame(self):
        stack = f"{PERF_ROOT};miri.run_hot;miri.kernel;miri.check;miri.check;miri.check;miri.check"
        self.assertEqual(self.folded[stack], 2)

    def test_kernel_frames_collapse_into_one(self):
        stack = f"{PERF_ROOT};miri.kernel;miri.bottom_up;miri.bottom_up;miri.bottom_up;miri.bottom_up;miri_rt_block_alloc;malloc;[libc.so.6];[kernel]"
        self.assertEqual(self.folded[stack], 1)
        self.assertEqual(self.folded["binary-trees.mi;[ld-linux-x86-64.so.2];[kernel]"], 1)

    def test_no_stack_carries_a_path(self):
        self.assertFalse([stack for stack in self.folded if "/" in stack])

    def test_top_symbols_and_hot_functions_come_out_as_on_macos(self):
        top = diagnose.top_symbols(self.folded)
        self.assertEqual([t["symbol"] for t in top[:2]], ["[kernel]", "miri.check"])
        self.assertEqual(diagnose.hot_miri_functions(top), ["miri.check", "miri.Node.$drop", "miri.bottom_up"])

    def test_output_without_samples_folds_to_nothing(self):
        self.assertEqual(diagnose.folded_perf_stacks(""), {})


class PerfAvailabilityTest(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.paranoid = Path(self.scratch.name) / "perf_event_paranoid"

    def problem(self, level, perf="/usr/bin/perf"):
        if level is not None:
            self.paranoid.write_text(f"{level}\n")
        with mock.patch.object(diagnose.shutil, "which", return_value=perf):
            return diagnose.perf_problem(self.paranoid)

    def test_a_machine_without_perf_says_so(self):
        self.assertEqual(self.problem(-1, perf=None), "perf is not installed")

    def test_a_paranoid_kernel_is_named_with_its_level_and_the_remedy(self):
        problem = self.problem(3)
        self.assertIn("kernel.perf_event_paranoid is 3", problem)
        self.assertIn("2 or less", problem)

    def test_a_permissive_kernel_is_no_problem(self):
        self.assertIsNone(self.problem(2))
        self.assertIsNone(self.problem(-1))

    def test_an_unreadable_level_leaves_the_verdict_to_perf(self):
        self.assertIsNone(self.problem(None))


def answer(step):
    """A scripted `subprocess.run` result: the result itself, or a function
    that makes it, for a step that also leaves a file behind."""
    return step() if callable(step) else step


class LinuxSamplingTest(unittest.TestCase):
    """The Linux sampler never fails the bundle: every way perf can fail
    becomes the reason the profile is missing."""

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.work = Path(self.scratch.name)
        self.port = run.Built("miri", ["port"], None)

    def sample(self, runs):
        child = mock.Mock(pid=7)
        with (
            mock.patch.object(diagnose.platform, "system", return_value="Linux"),
            mock.patch.object(diagnose, "perf_problem", return_value=None),
            mock.patch.object(diagnose.subprocess, "Popen", return_value=child),
            mock.patch.object(diagnose.subprocess, "run", side_effect=lambda *args, **kwargs: answer(runs.pop(0))),
            mock.patch.object(diagnose, "demangled", side_effect=lambda text: (text, "")),
            mock.patch.object(diagnose.time, "sleep"),
        ):
            result = diagnose.sample_profile(self.port, Path("input"), {}, self.work)
        child.kill.assert_called_once()
        return result

    def finished(self, returncode=0, stdout="", stderr=""):
        return subprocess.CompletedProcess([], returncode, stdout, stderr)

    def recorded(self, returncode=0, stderr=""):
        (self.work / "perf.7.data").write_text("")
        return self.finished(returncode, stderr=stderr)

    def test_a_refused_recording_names_perf_and_its_error(self):
        refused = self.finished(255, stderr="Error:\nAccess to performance monitoring and observability operations is limited.\n")
        reason = self.sample([refused])
        self.assertIn("perf record failed", reason)
        self.assertIn("Access to performance monitoring", reason)

    def test_a_recording_without_samples_says_so(self):
        reason = self.sample([self.recorded, self.finished(stdout="")])
        self.assertEqual(reason, "perf recorded no samples of the hot run")

    def test_a_recording_becomes_the_report_and_the_folded_stacks(self):
        script = (FIXTURES / "perf_script.binary-trees.txt").read_text()
        report = f"# report of {self.work}/bin/binary-trees.miri.profile\n"
        sampled = self.sample([self.recorded, self.finished(stdout=script), self.finished(stdout=report)])
        self.assertEqual(sum(sampled.folded.values()), 10)
        self.assertNotIn(str(self.work), sampled.report)
        self.assertIn("binary-trees.miri.profile", sampled.report)

    def test_a_hot_run_that_ends_inside_the_window_keeps_its_samples(self):
        # perf exits 255 when the process it is attached to exits first.
        script = (FIXTURES / "perf_script.binary-trees.txt").read_text()
        ended = lambda: self.recorded(255, stderr=f"[ perf record: Captured and wrote 0.160 MB {self.work}/perf.7.data (596 samples) ]")  # noqa: E731
        sampled = self.sample([ended, self.finished(stdout=script), self.finished(stdout="")])
        self.assertEqual(sum(sampled.folded.values()), 10)

    def test_a_refusal_after_the_data_file_exists_still_names_perfs_error(self):
        refused = lambda: self.recorded(255, stderr=f"Error:\nNo permission to write {self.work}/perf.7.data\n")  # noqa: E731
        reason = self.sample([refused, self.finished(stdout="")])
        self.assertIn("perf record failed", reason)
        self.assertIn("No permission to write <work>/perf.7.data", reason)

    def test_a_system_without_a_supported_profiler_says_which(self):
        with mock.patch.object(diagnose.platform, "system", return_value="Plan9"):
            self.assertEqual(diagnose.sample_profile(self.port, Path("input"), {}, self.work), "no sampling profiler is supported on Plan9")


class BuildLabelTest(unittest.TestCase):
    """DIAGNOSIS.md names the build every artifact of the bundle came from."""

    EXPECTED = {
        "profile.<language>.txt": "profile",
        "folded.<language>.txt": "profile",
        "flamegraph.<language>.svg": "profile",
        "top.<language>.json": "profile",
        "counters.json": "release",
        "miri.mir": "release",
        "miri.clif": "release",
        "hot.clif": "release",
        "hot.asm": "release",
        "runtime.json": "counted",
    }

    def markdown(self, profiles):
        workload = SimpleNamespace(name="binary-trees", manifest={"tags": []})
        return diagnose.diagnosis_markdown(workload, "s", {"cells": []}, profiles, {}, {})

    def test_every_artifact_is_named_with_its_build(self):
        rows = [line for line in self.markdown({}).splitlines() if line.startswith("| `")]
        for artifact, build in self.EXPECTED.items():
            row = next((line for line in rows if f"`{artifact}`" in line), None)
            self.assertIsNotNone(row, artifact)
            self.assertEqual(row.split("|")[2].strip(), build, artifact)

    def test_a_missing_profile_is_reported_with_its_reason(self):
        text = self.markdown({"miri": [{"symbol": "miri.check", "share": 1.0}], "c": "perf is not installed"})
        self.assertIn("**miri** (profile build): `miri.check` 100%", text)
        self.assertIn("**c**: unavailable: perf is not installed", text)


class DisassemblerTest(unittest.TestCase):
    """GNU objdump takes one `--disassemble=<symbol>` at a time; llvm-objdump
    takes the list in one `--disassemble-symbols=`."""

    def test_llvm_objdump_disassembles_every_symbol_in_one_command(self):
        commands = diagnose.disassembly_commands("llvm-objdump", "llvm", Path("hot"), ["miri.check", "miri.Node.$drop"])
        self.assertEqual(commands, [["llvm-objdump", "-d", "--disassemble-symbols=miri.check,miri.Node.$drop", "hot"]])

    def test_gnu_objdump_disassembles_one_symbol_per_command(self):
        commands = diagnose.disassembly_commands("objdump", "gnu", Path("hot"), ["miri.check", "miri.Node.$drop"])
        self.assertEqual(commands, [["objdump", "-d", "--disassemble=miri.check", "hot"], ["objdump", "-d", "--disassemble=miri.Node.$drop", "hot"]])

    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.bin = Path(self.scratch.name)

    def executable(self, name, version_text=""):
        path = self.bin / name
        path.write_text(f"#!/bin/sh\necho '{version_text}'\n")
        path.chmod(0o755)
        return str(path)

    def chosen(self):
        with mock.patch.dict(os.environ, {"PATH": str(self.bin)}):
            return diagnose.disassembler()

    def test_a_versioned_llvm_objdump_is_preferred_to_gnu_objdump(self):
        self.executable("objdump", "GNU objdump (GNU Binutils) 2.42")
        self.executable("llvm-objdump-17")
        newest = self.executable("llvm-objdump-18")
        self.assertEqual(self.chosen(), (newest, "llvm"))

    def test_gnu_objdump_is_recognised_by_its_version(self):
        gnu = self.executable("objdump", "GNU objdump (GNU Binutils) 2.42")
        self.assertEqual(self.chosen(), (gnu, "gnu"))

    def test_an_objdump_that_is_not_gnu_takes_the_llvm_flags(self):
        apple = self.executable("objdump", "Apple LLVM version 17.0.0")
        self.assertEqual(self.chosen(), (apple, "llvm"))

    def test_no_objdump_at_all_is_none(self):
        self.assertIsNone(self.chosen())


RUST_V0 = "_RINvNtCscI6d9CVNmLh_4core3ptr9drop_glueINtNtCs40k4W9msRzi_5alloc5boxed3BoxNtCsd4Lm9SSGt2z_12binary_trees4NodeEEB2_"


class DemangleTest(unittest.TestCase):
    def test_text_goes_through_cxxfilt_when_it_is_installed(self):
        done = subprocess.CompletedProcess([], 0, "core::ptr::drop_glue::<Node> (rust)\n", "")
        with mock.patch.object(diagnose.shutil, "which", return_value="/usr/bin/c++filt"), mock.patch.object(diagnose.subprocess, "run", return_value=done) as filt:
            text, note = diagnose.demangled(f"{RUST_V0} (rust)\n")
        self.assertEqual((text, note), ("core::ptr::drop_glue::<Node> (rust)\n", ""))
        self.assertEqual(filt.call_args.kwargs["input"], f"{RUST_V0} (rust)\n")

    def test_without_cxxfilt_symbols_stay_mangled_and_the_note_says_so(self):
        with mock.patch.object(diagnose.shutil, "which", return_value=None):
            text, note = diagnose.demangled(RUST_V0)
        self.assertEqual(text, RUST_V0)
        self.assertEqual(note, "symbols are left mangled: c++filt is not installed")

    def test_a_failing_cxxfilt_leaves_the_text_as_it_was(self):
        done = subprocess.CompletedProcess([], 1, "", "boom")
        with mock.patch.object(diagnose.shutil, "which", return_value="/usr/bin/c++filt"), mock.patch.object(diagnose.subprocess, "run", return_value=done):
            text, note = diagnose.demangled(RUST_V0)
        self.assertEqual(text, RUST_V0)
        self.assertEqual(note, "symbols are left mangled: c++filt failed")

    def test_the_linux_fold_and_report_are_demangled(self):
        script = (FIXTURES / "perf_script.binary-trees.txt").read_text().replace("miri.check", RUST_V0)
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.work = Path(scratch.name)
        steps = [self.record, subprocess.CompletedProcess([], 0, script, ""), subprocess.CompletedProcess([], 0, f"  50% {RUST_V0}\n", "")]
        with (
            mock.patch.object(diagnose.platform, "system", return_value="Linux"),
            mock.patch.object(diagnose, "perf_problem", return_value=None),
            mock.patch.object(diagnose.subprocess, "Popen", return_value=mock.Mock(pid=7)),
            mock.patch.object(diagnose.subprocess, "run", side_effect=lambda *args, **kwargs: answer(steps.pop(0))),
            mock.patch.object(diagnose, "demangled", side_effect=lambda text: (text.replace(RUST_V0, "drop_glue<Node>"), "")),
            mock.patch.object(diagnose.time, "sleep"),
        ):
            sampled = diagnose.sample_profile(run.Built("rust", ["port"], None), Path("input"), {}, self.work)
        self.assertIn("drop_glue<Node>", sampled.report)
        self.assertFalse([stack for stack in sampled.folded if RUST_V0 in stack])
        self.assertTrue([stack for stack in sampled.folded if stack.endswith(";drop_glue<Node>")])

    def record(self):
        (self.work / "perf.7.data").write_text("")
        return subprocess.CompletedProcess([], 0, "", "")


class BundlePrivacyTest(unittest.TestCase):
    """No artifact of a bundle names the machine's home, scratch or checkout
    directory, whichever step wrote it."""

    def test_a_bundle_written_under_a_home_directory_names_no_path_of_it(self):
        scratch = tempfile.TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        home = Path(scratch.name) / "home" / "someone"
        work = home / "tmp" / "miri-diagnose-x"
        out_dir = home / "results" / "diagnosis" / "nbody-s"
        (work / "bin").mkdir(parents=True)
        listing = work / "bin" / "nbody.listing"
        Path(f"{listing}.mir").write_text(f"// source {run.REPO}/bench/workloads/nbody/nbody.mi\n")
        Path(f"{listing}.clif").write_text(f"; miri.advance\nfunction %a() ; {home}/x\n")
        built = run.Built("miri", [str(work / "bin" / "nbody")], None)
        ports = diagnose.Ports({"miri": built}, {"miri": built}, listing)
        sampled = diagnose.Sampled(f"Path: {work}/bin/nbody\n", {"nbody;miri.advance": 3})
        workload = SimpleNamespace(name="nbody", manifest={"tags": []})
        with (
            mock.patch.object(diagnose.Path, "home", return_value=home),
            mock.patch.object(diagnose, "build_ports", return_value=ports),
            mock.patch.object(diagnose, "counters", return_value={"binary": f"{work}/bin/nbody"}),
            mock.patch.object(diagnose, "sample_profile", return_value=sampled),
            mock.patch.object(diagnose, "runtime_counts", return_value={"unavailable": f"{home}/build.log"}),
            mock.patch.object(diagnose, "disassembly", return_value=f"{work}/bin/nbody.listing: file format\n"),
            mock.patch.object(run, "input_file", return_value=work / "input"),
        ):
            diagnose.diagnose_cell(SimpleNamespace(work_dir=work), {"cells": []}, workload, "s", out_dir)
        written = {path.name: path.read_text() for path in out_dir.iterdir()}
        self.assertIn("miri.mir", written)
        leaks = {name for name, text in written.items() if str(home) in text or str(run.REPO) in text or scratch.name in text}
        self.assertEqual(leaks, set())


if __name__ == "__main__":
    unittest.main()
