# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/summarize.py, on synthetic records: noise flags,
verdicts at the charter's thresholds, ratios that only ever use measured
cells, and a summary that validates against its committed schema."""

import json
import sys
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parents[3] / "bench"
sys.path.insert(0, str(BENCH))

import run  # noqa: E402
import summarize  # noqa: E402

CHARTER = run.load_charter()
HASH = "h" * 64
MANIFESTS = {"w": {"tags": ["float"]}, "v": {"tags": ["int"]}}


def records(workload, language, walls, mode="cold", attempt=0, status="ok", rss=10_000_000):
    out = []
    for i, wall in enumerate(walls):
        out.append(
            {
                "schema_version": 1,
                "charter_version": 1,
                "charter_hash": HASH,
                "run_id": "first-run",
                "machine_profile": "dev",
                "workload": workload,
                "language": language,
                "variant": "default",
                "size": "s",
                "mode": mode,
                "run_index": i,
                "attempt": attempt,
                "status": status,
                "process_wall_ns": wall,
                "user_ns": wall,
                "sys_ns": 0,
                "max_rss_bytes": rss,
                "hot_ns": wall if mode == "hot" else None,
                "floor_rss_bytes": 1_000_000,
                "binary_bytes": 100,
                "startup_ns": 1000,
            }
        )
    return out


def unmeasured(workload, language, status):
    record = records(workload, language, [1])[0]
    record.update(run_index=None, attempt=None, status=status)
    return record


def steady(median, cv, n=21):
    """`n` values with the given median whose spread has roughly `cv`."""
    half = n // 2
    step = median * cv * 1.7 / half
    return [int(median + (i - half) * step) for i in range(n)]


def summarize_records(recs):
    return summarize.summarize(recs, CHARTER, HASH, MANIFESTS)


def cell(summary, workload, language, mode="cold"):
    return next(c for c in summary["cells"] if (c["workload"], c["language"], c["mode"]) == (workload, language, mode))


class NoiseTest(unittest.TestCase):
    def test_a_cell_past_the_cv_threshold_is_noisy(self):
        summary = summarize_records(records("w", "c", steady(100, 0.01)) + records("w", "miri", steady(200, 0.2)))
        self.assertEqual(cell(summary, "w", "c")["status"], "ok")
        self.assertEqual(cell(summary, "w", "miri")["status"], "noisy")

    def test_only_the_last_attempt_is_summarized(self):
        recs = records("w", "c", steady(100, 0.3), attempt=0) + records("w", "c", steady(100, 0.01), attempt=1)
        c = cell(summarize_records(recs), "w", "c")
        self.assertEqual(c["status"], "ok")
        self.assertEqual(c["attempts"], 2)


class RatioTest(unittest.TestCase):
    def test_ratios_are_medians_over_cs_and_net_memory_subtracts_the_floor(self):
        summary = summarize_records(records("w", "c", [100, 100, 100]) + records("w", "miri", [150, 150, 150], rss=21_000_000))
        miri = cell(summary, "w", "miri")
        self.assertAlmostEqual(miri["vs_c"]["time"]["value"], 1.5)
        self.assertEqual(miri["metrics"]["net_rss_bytes"]["value"], 20_000_000)
        self.assertTrue(miri["diagnose"])

    def test_a_mismatch_or_timeout_never_enters_a_ratio(self):
        recs = records("w", "c", [100, 100, 100]) + [unmeasured("w", "miri", "mismatch")]
        recs += records("v", "c", [100, 100]) + records("v", "miri", [100, 99999], status="timeout")
        summary = summarize_records(recs)
        self.assertEqual(cell(summary, "w", "miri")["status"], "mismatch")
        self.assertNotIn("vs_c", cell(summary, "w", "miri"))
        self.assertEqual(cell(summary, "v", "miri")["status"], "timeout")
        miri_all = [a for a in summary["aggregates"] if a["language"] == "miri" and a["tag"] == "all"]
        self.assertEqual(miri_all, [])

    def test_the_geometric_mean_counts_only_workloads_with_a_ratio(self):
        recs = records("w", "c", [100, 100, 100]) + records("w", "miri", [200, 200, 200])
        recs += records("v", "c", [100, 100, 100]) + records("v", "miri", [50, 50, 50])
        recs += [unmeasured("u", "miri", "unsupported")]
        summary = summarize_records(recs)
        overall = next(a for a in summary["aggregates"] if a["language"] == "miri" and a["tag"] == "all")
        self.assertAlmostEqual(overall["geomean_vs_c"], 1.0)
        self.assertEqual(overall["coverage"], 2)


class VerdictTest(unittest.TestCase):
    def summary_with(self, miri_median, cv, run_id):
        recs = records("w", "c", steady(1000, cv)) + records("w", "miri", steady(miri_median, cv))
        for r in recs:
            r["run_id"] = run_id
        return summarize_records(recs)

    def test_an_eight_percent_slowdown_at_one_percent_cv_is_slower(self):
        rows = summarize.compare(self.summary_with(1000, 0.01, "a"), self.summary_with(1080, 0.01, "b"), CHARTER)["rows"]
        self.assertEqual(next(r for r in rows if r["language"] == "miri")["verdict"], "slower")

    def test_the_same_slowdown_at_four_percent_cv_is_the_same(self):
        rows = summarize.compare(self.summary_with(1000, 0.04, "a"), self.summary_with(1080, 0.04, "b"), CHARTER)["rows"]
        self.assertEqual(next(r for r in rows if r["language"] == "miri")["verdict"], "same")

    def test_two_profiles_are_never_compared(self):
        a, b = self.summary_with(1000, 0.01, "a"), self.summary_with(1000, 0.01, "b")
        b["machine_profile"] = "publication"
        with self.assertRaises(SystemExit):
            summarize.compare(a, b, CHARTER)


class ProvenanceTest(unittest.TestCase):
    def test_records_under_another_charter_are_refused(self):
        recs = records("w", "c", [100])
        with self.assertRaises(SystemExit):
            summarize.summarize(recs, CHARTER, "other", MANIFESTS)

    def test_mixed_profiles_are_refused(self):
        recs = records("w", "c", [100]) + records("w", "miri", [100])
        recs[-1]["machine_profile"] = "publication"
        with self.assertRaises(SystemExit):
            summarize_records(recs)


class StatisticsTest(unittest.TestCase):
    def test_nearest_rank_quantile_is_an_observed_value(self):
        self.assertEqual(summarize.nearest_rank([5, 1, 4, 2, 3], 0.9), 5)
        self.assertEqual(summarize.nearest_rank([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 0.9), 9)


def validate(instance, schema, root):
    """The subset of JSON Schema the summary schema uses: type, const, enum,
    required, properties, items and local $ref."""
    if "$ref" in schema:
        name = schema["$ref"].split("/")[-1]
        return validate(instance, root["$defs"][name], root)
    kinds = {"object": dict, "array": list, "string": str, "integer": int, "number": (int, float), "boolean": bool, "null": type(None)}
    if "type" in schema:
        allowed = schema["type"] if isinstance(schema["type"], list) else [schema["type"]]
        assert any(isinstance(instance, kinds[k]) for k in allowed), (instance, allowed)
    if "const" in schema:
        assert instance == schema["const"], (instance, schema["const"])
    if "enum" in schema:
        assert instance in schema["enum"], (instance, schema["enum"])
    for key in schema.get("required", []):
        assert key in instance, f"missing {key}"
    for key, sub in schema.get("properties", {}).items():
        if isinstance(instance, dict) and key in instance:
            validate(instance[key], sub, root)
    if "items" in schema and isinstance(instance, list):
        for item in instance:
            validate(item, schema["items"], root)


class SchemaTest(unittest.TestCase):
    def test_a_summary_validates_against_the_committed_schema(self):
        schema = json.loads((BENCH / "schema" / "summary.v1.json").read_text())
        recs = records("w", "c", [100, 100, 100]) + records("w", "miri", steady(150, 0.01))
        recs += records("w", "c", [50, 50, 50], mode="hot") + [unmeasured("v", "miri", "mismatch")]
        validate(summarize_records(recs), schema, schema)


if __name__ == "__main__":
    unittest.main()
