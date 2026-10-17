# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/run.py: what it measures, how it reads what programs
print, the order it runs them in, and the records it writes."""

import argparse
import hashlib
import sys
import tempfile
import unittest
from unittest import mock
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3] / "bench"))

import run  # noqa: E402

# The fields schema version 1 promises on every measured run.
MEASURED_FIELDS = {
    "schema_version",
    "charter_version",
    "charter_hash",
    "run_id",
    "machine_profile",
    "cpu_model",
    "os",
    "governor",
    "turbo",
    "image_digest",
    "runner_commit",
    "miri_sha",
    "miri_version",
    "toolchain_versions",
    "runtime",
    "workload",
    "workload_hash",
    "manifest_sha256",
    "language",
    "variant",
    "size",
    "mode",
    "run_index",
    "attempt",
    "process_wall_ns",
    "user_ns",
    "sys_ns",
    "max_rss_bytes",
    "hot_ns",
    "hot_ns_samples",
    "floor_rss_bytes",
    "startup_ns",
    "binary_bytes",
    "status",
    "exit_code",
}


def fake_environment():
    return {
        "charter_version": 1,
        "charter_hash": "c" * 64,
        "run_id": "test",
        "machine_profile": "dev",
        "cpu_model": "cpu",
        "os": "os",
        "governor": None,
        "turbo": None,
        "image_digest": None,
        "runner_commit": "r",
        "miri_sha": "m",
        "miri_version": "miri 0",
        "toolchain_versions": {},
    }


def context(work_dir, runs=1):
    args = argparse.Namespace(runs=runs, variant="default", sizes="s", smoke=False, langs=None, profile="dev")
    return run.Context(args, run.load_charter(), "c" * 64, "test", Path(work_dir), Path("miri"))


def script_port(work_dir, name, printed):
    """A port that prints `printed` and, given `hot`, one hot_ns line."""
    path = Path(work_dir) / f"{name}.py"
    path.write_text(
        "import sys\n"
        "if len(sys.argv) > 2:\n"
        "    print('hot_ns=1000', file=sys.stderr)\n"
        f"print({printed!r})\n"
    )
    return run.Built("python" if name == "right" else "typescript", [sys.executable, str(path)], None)


class RusageTest(unittest.TestCase):
    def test_linux_reports_kibibytes_and_macos_bytes(self):
        self.assertEqual(run.max_rss_bytes(2048, "Linux"), 2048 * 1024)
        self.assertEqual(run.max_rss_bytes(2048, "Darwin"), 2048)

    def test_a_real_child_reports_its_usage(self):
        measurement = run.measure([sys.executable, "-c", "print('x' * 3)"], None, None, 60)
        self.assertEqual(measurement.exit_code, 0)
        self.assertEqual(measurement.stdout, "xxx\n")
        self.assertGreater(measurement.process_wall_ns, 0)
        self.assertGreater(measurement.max_rss_bytes, 1024 * 1024)

    def test_a_run_past_its_cap_is_killed(self):
        measurement = run.measure([sys.executable, "-c", "import time; time.sleep(30)"], None, None, 0.5)
        self.assertTrue(measurement.timed_out)


class StderrTest(unittest.TestCase):
    def test_every_hot_line_is_read_in_order_and_nothing_else(self):
        stderr = "warming up\nhot_ns=30\n  hot_ns=10  \nhot_ns=abc\nnoise hot_ns=5\nhot_ns=20\n"
        self.assertEqual(run.hot_samples(stderr), [30, 10, 20])

    def test_no_hot_lines_is_an_empty_list(self):
        self.assertEqual(run.hot_samples(""), [])

    def test_the_allocation_total_is_read_from_the_counter_line(self):
        stderr = "hot_ns=5\nMIRI_ALLOC_COUNT: 42 allocation(s) (rc=8 inline=4 buffers=23 raw=7)\n"
        self.assertEqual(run.allocation_count(stderr), 42)
        self.assertIsNone(run.allocation_count("nothing here\n"))

    def test_the_runtime_counters_become_record_fields(self):
        stderr = (
            "MIRI_ALLOC_COUNT: 42 allocation(s) (rc=8 inline=4 buffers=23 raw=7)\n"
            "MIRI_RC_STATS: dup=10 drop=18 drop_free=12 cow_copy=1 realloc=5\n"
        )
        self.assertEqual(
            run.runtime_counters(stderr),
            {
                "alloc_rc": 8,
                "alloc_inline": 4,
                "alloc_buffer": 23,
                "alloc_raw": 7,
                "rc_dup": 10,
                "rc_drop": 18,
                "rc_drop_free": 12,
                "cow_copy": 1,
                "realloc": 5,
            },
        )


class OrderTest(unittest.TestCase):
    def test_languages_interleave_within_every_round(self):
        self.assertEqual(
            run.interleaved(["miri", "c"], 3),
            [(0, "miri"), (0, "c"), (1, "miri"), (1, "c"), (2, "miri"), (2, "c")],
        )


class CommandTest(unittest.TestCase):
    def test_placeholders_programs_and_leading_assignments(self):
        words, assignments = run.command_words(
            "GOAMD64=v3 go build -o {out} {src}", Path("/s.go"), Path("/o"), {"go": Path("/bin/go")}
        )
        self.assertEqual(words, ["/bin/go", "build", "-o", "/o", "/s.go"])
        self.assertEqual(assignments, {"GOAMD64": "v3"})

    def test_the_child_environment_carries_nothing_unlisted(self):
        env = run.child_environment(Path("/w"))
        self.assertEqual(set(env), {"PATH", "HOME", "PYTHONHASHSEED", "DENO_DIR"})
        self.assertEqual(env["PYTHONHASHSEED"], "0")


class CountedBuildTest(unittest.TestCase):
    def test_the_counted_build_is_the_timed_line_without_release(self):
        ctx = argparse.Namespace(charter=run.load_charter(), work_dir=Path("/work"))
        with mock.patch.object(run, "build_with_line", return_value="build-failed") as build:
            status = run.counted_build(ctx, "nbody", Path("nbody.mi"))
        self.assertEqual(status, "build-failed")
        self.assertEqual(ctx.charter["languages"]["miri"]["build"], "miri build --release --opt-level 3 -o {out} {src}")
        build.assert_called_once_with(
            ctx, "miri", "miri build --opt-level 3 -o {out} {src}", Path("nbody.mi"), Path("/work/bin/nbody.miri.counted")
        )


class ReviewTest(unittest.TestCase):
    def test_the_hash_is_of_the_file_without_its_review_line(self):
        body = "// header\n\nint main() {}\n"
        digest = hashlib.sha256(body.encode()).hexdigest()
        lines = body.split("\n")
        reviewed = "\n".join(lines[:1] + [f"// port-checklist: reviewed 2026-01-01 by r sha256={digest}"] + lines[1:])
        review, computed = run.review_hash(reviewed, "//")
        self.assertEqual(computed, digest)
        self.assertTrue(review.endswith(digest))

    def test_every_committed_port_passes_its_review(self):
        charter = run.load_charter()
        for workload in run.load_workloads(None):
            for language in ("miri", "c", "cpp", "rust", "go", "python", "typescript"):
                comment = charter["languages"][language]["checklist_comment"]
                self.assertIsNone(run.review_problem(workload.port(language), comment), workload.port(language))


class ProfileTest(unittest.TestCase):
    def test_every_profile_names_itself_and_only_publication_publishes(self):
        for name in ("dev", "ci-smoke", "publication"):
            profile = run.load_profile(name)
            self.assertEqual(profile["name"], name)
            self.assertEqual(profile["publishable"], name == "publication")

    def test_a_profile_without_a_pinned_core_pins_nothing(self):
        self.assertEqual(run.pin_words(run.load_profile("dev")), [])

    def test_this_machine_is_refused_for_publication_unless_it_is_prepared(self):
        problems = run.publication_problems(run.load_profile("publication"))
        if run.platform.system() != "Linux":
            self.assertIn("a publication run needs Linux", problems)

    def test_inputs_go_to_the_profiles_directory_when_it_exists(self):
        with tempfile.TemporaryDirectory() as root:
            ctx = context(root)
            ctx.profile = {"input_dir": root}
            self.assertEqual(run.inputs_dir(ctx).parent, Path(root))
            ctx.profile = {"input_dir": "/no/such/directory"}
            self.assertEqual(run.inputs_dir(ctx).parent, Path(root))


class StatisticsTest(unittest.TestCase):
    def test_coefficient_of_variation(self):
        self.assertEqual(run.coefficient_of_variation([5, 5, 5]), 0.0)
        self.assertAlmostEqual(run.coefficient_of_variation([9, 11]), 0.1414, places=3)
        self.assertEqual(run.coefficient_of_variation([7]), 0.0)


class RecordTest(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        self.ctx = context(self.dir.name)
        self.workload = run.Workload("fake", {"sizes": {"s": {"parameter": 1, "checksum": "42"}}}, "f" * 64)

    def tearDown(self):
        self.dir.cleanup()

    def run_size(self, ready):
        run.run_workload_size(self.ctx, fake_environment(), self.workload, ready, "s", {})
        return self.ctx.records

    def test_a_measured_run_carries_every_schema_field(self):
        records = self.run_size({"python": script_port(self.dir.name, "right", "42")})
        measured = [r for r in records if r["status"] == "ok"]
        self.assertTrue(measured)
        for record in measured:
            self.assertEqual(MEASURED_FIELDS - set(record), set())
        hot = [r for r in measured if r["mode"] == "hot"]
        self.assertTrue(hot)
        self.assertEqual(hot[0]["hot_ns"], 1000)

    def test_a_wrong_checksum_is_a_mismatch_and_is_never_timed(self):
        records = self.run_size(
            {"python": script_port(self.dir.name, "right", "42"), "typescript": script_port(self.dir.name, "wrong", "41")}
        )
        wrong = [r for r in records if r["language"] == "typescript"]
        self.assertEqual([r["status"] for r in wrong], ["mismatch"])
        self.assertNotIn("process_wall_ns", wrong[0])
        self.assertTrue(any(r["language"] == "python" and r["status"] == "ok" for r in records))

    def test_measured_runs_follow_the_warm_up(self):
        self.ctx.args.runs = 3
        records = self.run_size({"python": script_port(self.dir.name, "right", "42")})
        cold = [r for r in records if r["mode"] == "cold" and r["attempt"] == 0]
        self.assertEqual([r["run_index"] for r in cold], [0, 1, 2])


if __name__ == "__main__":
    unittest.main()
