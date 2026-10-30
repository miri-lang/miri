# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/history.py and the version line it feeds: only a
publishable profile records a point, a port a release cannot build is kept as
unsupported with its diagnostic, each point's env.json names its tag, commit
and code-generator optimisation, and summarize.py and plot.py read the points
in version order with the current tree last."""

import argparse
import importlib.util
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parents[3] / "bench"
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(Path(__file__).resolve().parent))

import history  # noqa: E402
import run  # noqa: E402
import summarize  # noqa: E402
from test_summarize import records, summarize_records, unmeasured  # noqa: E402

HAS_MATPLOTLIB = importlib.util.find_spec("matplotlib") is not None
PROFILE = {"label": "preliminary", "publishable": True}


def has_revision(revision):
    done = subprocess.run(["git", "rev-parse", "--verify", "--quiet", f"{revision}^{{commit}}"], cwd=run.REPO, capture_output=True, check=False)
    return done.returncode == 0


def tag_point(tag, commit_time, miri_walls, optimized=False, missing=()):
    """A tag's summary: its Miri records, one port it could not build, and the
    session's C references adopted under its run id."""
    reference = records("w", "c", [100, 100, 100]) + records("v", "c", [100, 100, 100])
    for record in reference:
        record["run_id"] = "current-run"
    own = []
    for workload, walls in miri_walls.items():
        own += records(workload, "miri", walls)
    for workload in missing:
        failed = unmeasured(workload, "miri", "build-failed")
        failed["reason"] = "error[MER_PAR_001]: unexpected token · at w.mi line 3"
        own.append(failed)
    for record in own:
        record["run_id"] = f"run-{tag}"
    combined = history.with_references(history.mark_unsupported(own, tag), reference, "f" * 40)
    summary = summarize_records(combined)
    facts = {"commit": "f" * 40, "commit_time": commit_time, "codegen_optimized": optimized, "runtime_profile": {"name": "release"}}
    summary["provenance"] = history.point_environment({"miri_version": f"miri {tag}"}, facts, tag, PROFILE, "s", tag, "current-run")
    return summary


def current_point(miri_walls):
    recs = records("w", "c", [100, 100, 100]) + records("v", "c", [100, 100, 100])
    for workload, walls in miri_walls.items():
        recs += records(workload, "miri", walls)
    for record in recs:
        record["run_id"] = "current-run"
    summary = summarize_records(recs)
    facts = {"commit": "a" * 40, "commit_time": "2026-10-29T00:00:00+01:00", "codegen_optimized": True, "runtime_profile": {"name": "release"}}
    summary["provenance"] = history.point_environment({"miri_version": "miri 0.8.0"}, facts, "aaaaaaaa (current)", PROFILE, "s")
    return summary


def points():
    """Two releases and the current tree, handed over out of order."""
    return [
        current_point({"w": [120, 120, 120], "v": [110, 110, 110]}),
        tag_point("0.7.0-beta.5", "2026-10-15T21:52:43+02:00", {"w": [300, 300, 300], "v": [250, 250, 250]}),
        tag_point("0.6.0-beta.4", "2026-08-23T21:58:06+02:00", {"w": [400, 400, 400]}, missing=("v",)),
    ]


class ProfileTest(unittest.TestCase):
    def test_a_profile_that_may_not_publish_records_no_point(self):
        with self.assertRaises(SystemExit) as refused:
            history.publishable_profile("dev")
        self.assertIn("only a publishable profile", str(refused.exception))

    def test_an_unknown_profile_is_refused(self):
        with self.assertRaises(SystemExit):
            history.publishable_profile("no-such-machine")

    def test_the_vps_profile_records_points_under_its_label(self):
        self.assertEqual(history.publishable_profile("preliminary-vps")["label"], "preliminary")
        self.assertTrue(history.publishable_profile("publication")["publishable"])


class UnsupportedTest(unittest.TestCase):
    def test_a_port_the_tag_cannot_build_is_unsupported_with_its_diagnostic(self):
        failed = unmeasured("v", "miri", "build-failed")
        failed["reason"] = "error[MER_PAR_001]: unexpected token · at v.mi line 3"
        timed_out = unmeasured("u", "miri", "timeout")
        marked = history.mark_unsupported([failed, timed_out], "0.6.0-beta.4")
        self.assertEqual(marked[0]["status"], "unsupported")
        self.assertEqual(marked[0]["reason"], "unsupported (0.6.0-beta.4): error[MER_PAR_001]: unexpected token · at v.mi line 3")
        self.assertEqual(marked[1]["status"], "timeout")

    def test_the_first_diagnostic_names_its_code_and_line_and_no_directory(self):
        stderr = (
            "warning: unused variable\n"
            "error[MER_TYP_012]: no method `push` on /home/someone/hist/tags/0.6.0-beta.4/src/stdlib/x.mi\n"
            "  --> /home/someone/miri/bench/workloads/w/w.mi:14:9\n"
        )
        reason = run.first_diagnostic(stderr)
        self.assertIn("MER_TYP_012", reason)
        self.assertIn("w.mi line 14", reason)
        self.assertNotIn("/home", reason)

    def test_the_diagnostic_keeps_the_line_that_explains_it(self):
        stderr = (
            "error[MER_TYP_033]: Field Not Found\n"
            "--> /home/someone/hist/head/bench/workloads/sieve/sieve.mi:18:5\n"
            "    |\n"
            " 18 |     composite.resize(n + 1, false)\n"
            "    |     ^^^^^^^^^^^^^^^^ Type 'List' has no field or method 'resize'\n"
        )
        self.assertEqual(
            run.first_diagnostic(stderr),
            "error[MER_TYP_033]: Field Not Found · Type 'List' has no field or method 'resize' · at sieve.mi line 18",
        )
        generated = "error[MER_CG_005]: Translation Error\n  = Failed to translate function 'f'\n  = help: Please report this\n"
        self.assertEqual(run.first_diagnostic(generated), "error[MER_CG_005]: Translation Error · Failed to translate function 'f'")

    def test_a_build_failure_record_carries_the_diagnostic(self):
        workload = run.Workload("w", {}, "f" * 64)
        args = argparse.Namespace(runs=1, variant="default", sizes="s", smoke=False, langs=None, profile="dev")
        ctx = run.Context(args, run.load_charter(), "c" * 64, "test", Path("/w"), Path("miri"))
        ctx.build_errors[workload.port("miri")] = "error: E · at w.mi line 1"
        env = {key: None for key in ("charter_version", "charter_hash", "run_id", "machine_profile", "cpu_model", "os", "governor", "turbo")}
        env |= {key: None for key in ("image_digest", "runner_commit", "miri_sha", "miri_version", "toolchain_versions")}
        record = run.status_record(ctx, env, workload, "miri", "default", "s", "build-failed")
        self.assertEqual(record["reason"], "error: E · at w.mi line 1")

    def test_unsupported_ports_are_listed_in_the_coverage_block(self):
        summary = tag_point("0.6.0-beta.4", "2026-08-23T21:58:06+02:00", {"w": [400, 400, 400]}, missing=("v",))
        miri = summary["coverage"]["miri"]
        self.assertEqual(miri["measured"], ["w"])
        self.assertEqual([(e["workload"], e["status"]) for e in miri["unmeasured"]], [("v", "unsupported")])
        self.assertIn("MER_PAR_001", miri["unmeasured"][0]["reason"])
        self.assertIn("unsupported (0.6.0-beta.4)", summarize.markdown(summary))


class EnvironmentTest(unittest.TestCase):
    def test_a_points_environment_names_the_tag_its_commit_and_the_codegen_fact(self):
        facts = {"commit": "b" * 40, "commit_time": "2026-08-23T21:58:06+02:00", "codegen_optimized": False, "runtime_profile": {"name": "release"}}
        env = history.point_environment({"run_id": "r", "miri_sha": "head"}, facts, "0.6.0-beta.4", PROFILE, "m", "0.6.0-beta.4", "ref")
        self.assertEqual((env["tag"], env["tag_sha"], env["miri_sha"]), ("0.6.0-beta.4", "b" * 40, "b" * 40))
        self.assertIs(env["codegen_optimized"], False)
        self.assertEqual(env["miri_runtime_profile"], {"name": "release"})
        self.assertEqual((env["label"], env["sizes"], env["reference_run_id"], env["point_label"]), ("preliminary", "m", "ref", "0.6.0-beta.4"))

    def test_the_current_tree_has_no_tag(self):
        facts = {"commit": "a" * 40, "commit_time": "t", "codegen_optimized": True, "runtime_profile": {"name": "release"}}
        env = history.point_environment({}, facts, "aaaaaaaa (current)", PROFILE, None)
        self.assertEqual((env["tag"], env["tag_sha"], env["sizes"]), (None, None, "s,m,l"))

    def test_adopted_references_keep_the_run_they_came_from(self):
        own = records("w", "miri", [1, 1])
        for record in own:
            record["run_id"] = "tag-run"
        adopted = history.with_references(own, records("w", "c", [1]) + records("w", "miri", [9]), "c" * 40)
        self.assertEqual({r["run_id"] for r in adopted}, {"tag-run"})
        self.assertEqual([r["language"] for r in adopted], ["miri", "miri", "c"])
        self.assertEqual(adopted[-1]["reference_run_id"], "first-run")
        self.assertEqual({r["miri_sha"] for r in adopted if r["language"] == "miri"}, {"c" * 40})

    @unittest.skipUnless(has_revision(history.CODEGEN_OPTIMIZATION_COMMIT), "a shallow clone without the commit")
    def test_the_codegen_fact_is_read_from_the_commit_graph(self):
        self.assertTrue(history.commit_facts(history.CODEGEN_OPTIMIZATION_COMMIT)["codegen_optimized"])
        self.assertFalse(history.commit_facts(f"{history.CODEGEN_OPTIMIZATION_COMMIT}~1")["codegen_optimized"])

    @unittest.skipUnless(has_revision("0.7.0-beta.5"), "a clone without the release tags")
    def test_a_release_records_the_runtime_profile_it_shipped_with(self):
        facts = history.commit_facts("0.7.0-beta.5")
        self.assertIs(facts["codegen_optimized"], False)
        self.assertEqual(facts["runtime_profile"], {"name": "release"})

    def test_the_session_command_measures_the_tags_compiler_with_its_stdlib(self):
        args = argparse.Namespace(profile="preliminary-vps", sizes="m", only=None, runs=None, allow_dirty=False)
        words = history.run_words(args, Path("/out"), ["miri"], Path("/t/target/release/miri"), Path("/t/src/stdlib"))
        self.assertEqual(words[words.index("--miri-stdlib") + 1], "/t/src/stdlib")
        self.assertIn("--allow-unoptimized", words)
        self.assertEqual(words[words.index("--langs") + 1], "miri")
        parsed = run.parse_args(words[2:])
        self.assertEqual((parsed.miri, parsed.miri_stdlib, parsed.results_dir), ("/t/target/release/miri", "/t/src/stdlib", "/out"))


class RunnerOverrideTest(unittest.TestCase):
    def test_the_compiler_reads_the_stdlib_it_was_given(self):
        args = argparse.Namespace(profile="dev")
        ctx = run.Context(args, {}, "h", "r", Path("/w"), Path("/t/miri"), stdlib=Path("/t/src/stdlib"))
        self.assertEqual(ctx.build_env("miri")["MIRI_STDLIB_PATH"], "/t/src/stdlib")
        self.assertNotIn("MIRI_STDLIB_PATH", ctx.build_env("c"))

    def test_a_requested_results_directory_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as root:
            out = Path(root) / "point"
            self.assertEqual(run.results_directory("dev", out), out)
            with self.assertRaises(SystemExit):
                run.results_directory("dev", out)


class HistoryTest(unittest.TestCase):
    def test_points_follow_the_commit_time_and_are_labelled_by_tag(self):
        series = summarize.history(points())
        w = next(s for s in series if s.get("workload") == "w" and s["language"] == "miri" and s["mode"] == "cold")
        self.assertEqual([p["label"] for p in w["points"]], ["0.6.0-beta.4", "0.7.0-beta.5", "aaaaaaaa (current)"])
        self.assertEqual([p["vs_c"] for p in w["points"]], [4.0, 3.0, 1.2])
        self.assertEqual([p["codegen_optimized"] for p in w["points"]], [False, False, True])

    def test_a_workload_missing_at_a_tag_keeps_its_place_on_the_line(self):
        series = summarize.history(points())
        v = next(s for s in series if s.get("workload") == "v" and s["language"] == "miri" and s["mode"] == "cold")
        self.assertEqual([p["index"] for p in v["points"]], [1, 2])

    def test_the_geometric_mean_is_a_series_with_its_coverage(self):
        series = summarize.history(points())
        mean = next(s for s in series if s.get("aggregate") == "geomean" and s["language"] == "miri" and s["mode"] == "cold")
        self.assertEqual([p["coverage"] for p in mean["points"]], [1, 2, 2])
        self.assertAlmostEqual(mean["points"][0]["vs_c"], 4.0)

    def test_a_release_that_builds_no_port_keeps_its_place_with_its_coverage(self):
        nothing = tag_point("0.5.0-beta.3", "2026-07-01T00:00:00+02:00", {}, missing=("w", "v"))
        runs = next(s for s in summarize.history([nothing, *points()]) if s.get("aggregate") == "runs")
        self.assertEqual([p["label"] for p in runs["points"]][0], "0.5.0-beta.3")
        self.assertEqual(runs["points"][0]["coverage"]["miri"], {"measured": 0, "unsupported": 2})
        self.assertEqual(runs["points"][1]["coverage"]["miri"], {"measured": 1, "unsupported": 1})

    def test_a_history_across_profiles_is_refused(self):
        mixed = points()
        mixed[0]["machine_profile"] = "publication"
        with self.assertRaises(SystemExit):
            summarize.history(mixed)


@unittest.skipUnless(HAS_MATPLOTLIB, "matplotlib is not installed")
class HistoryChartTest(unittest.TestCase):
    def test_the_chart_ends_on_the_current_tree_and_footnotes_the_unoptimised_tags(self):
        import plot

        summary = current_point({"w": [120, 120, 120], "v": [110, 110, 110]})
        with tempfile.TemporaryDirectory() as out:
            plot.plot(summary, out, history=summarize.history(points()), diagnosis_dir=Path(out) / "none")
            svg = (Path(out) / "miri-history.svg").read_text()
        self.assertLess(svg.index("0.6.0-beta.4"), svg.index("0.7.0-beta.5"))
        self.assertLess(svg.index("0.7.0-beta.5"), svg.index("aaaaaaaa (current)"))
        self.assertIn("0.6.0-beta.4, 0.7.0-beta.5: measured as released", svg)
        self.assertIn("4.00× (1)", svg)

    def test_a_release_that_builds_no_port_is_labelled_on_the_line(self):
        import plot

        nothing = tag_point("0.5.0-beta.3", "2026-07-01T00:00:00+02:00", {}, missing=("w", "v"))
        summary = current_point({"w": [120, 120, 120], "v": [110, 110, 110]})
        with tempfile.TemporaryDirectory() as out:
            plot.plot(summary, out, history=summarize.history([nothing, *points()]), diagnosis_dir=Path(out) / "none")
            svg = (Path(out) / "miri-history.svg").read_text()
        self.assertIn("0.5.0-beta.3", svg)
        self.assertIn("no port compiles", svg)
        self.assertIn("0.5.0-beta.3, 0.6.0-beta.4, 0.7.0-beta.5: measured as released", svg)


if __name__ == "__main__":
    unittest.main()
