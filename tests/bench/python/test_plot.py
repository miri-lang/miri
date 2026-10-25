# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Unit tests for bench/plot.py and bench/export_site.py, on a synthetic
summary that carries an unsupported, a timed-out and a noisy cell: the charts
draw the absent ones as labels, never as zero; the SVG is deterministic; and
the website check catches a single hand-edited number. Skipped when
matplotlib is not installed (bench/requirements.txt)."""

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path

BENCH = Path(__file__).resolve().parents[3] / "bench"
sys.path.insert(0, str(BENCH))
sys.path.insert(0, str(Path(__file__).resolve().parent))

HAS_MATPLOTLIB = importlib.util.find_spec("matplotlib") is not None

from test_summarize import records, steady, summarize_records, unmeasured  # noqa: E402


def fixture():
    """A publication summary: one measured workload, one Miri port that is
    unsupported, one that timed out and one that is noisy."""
    recs = records("w", "c", [100, 100, 100]) + records("w", "miri", [150, 150, 150])
    recs += records("w", "rust", [110, 110, 110])
    recs += records("v", "c", [100, 100, 100]) + [unmeasured("v", "miri", "unsupported")]
    recs += records("u", "c", [100, 100]) + records("u", "miri", [100, 99999], status="timeout")
    recs += records("n", "c", steady(100, 0.01)) + records("n", "miri", steady(200, 0.2))
    for record in recs:
        record["machine_profile"] = "publication"
    summary = summarize_records(recs)
    summary["provenance"] = {"miri_version": "miri 9.9.9 (test)", "cpu_model": "test cpu", "miri_sha": "0" * 40}
    return summary


@unittest.skipUnless(HAS_MATPLOTLIB, "matplotlib is not installed")
class PlotTest(unittest.TestCase):
    def setUp(self):
        import plot

        self.plot = plot
        self.summary = fixture()

    def draw(self, out):
        self.plot.plot(self.summary, out, diagnosis_dir=Path(out) / "none")
        return (Path(out) / "time-vs-c.svg").read_text()

    def test_unmeasured_cells_are_labelled_and_noise_is_in_the_legend(self):
        with tempfile.TemporaryDirectory() as out:
            svg = self.draw(out)
        self.assertIn("unsupported", svg)
        self.assertIn("timeout", svg)
        self.assertIn("noisy (CV over the charter", svg)

    def test_the_svg_is_identical_across_renders(self):
        with tempfile.TemporaryDirectory() as a, tempfile.TemporaryDirectory() as b:
            self.assertEqual(self.draw(a), self.draw(b))

    def test_the_footer_carries_the_provenance(self):
        with tempfile.TemporaryDirectory() as out:
            svg = self.draw(out)
        self.assertIn("test cpu", svg)
        self.assertIn("9.9.9", svg)

    def test_the_checks_chart_is_drawn_only_when_rust_checked_was_measured(self):
        with tempfile.TemporaryDirectory() as out:
            self.draw(out)
            self.assertFalse((Path(out) / "checks-cost.svg").exists())
        recs = records("w", "c", [100, 100, 100]) + records("w", "miri", [150, 150, 150])
        recs += records("w", "rust", [100, 100, 100]) + records("w", "rust-checked", [120, 120, 120])
        summary = summarize_records(recs)
        summary["provenance"] = {"miri_version": "miri 9.9.9 (test)", "cpu_model": "test cpu", "miri_sha": "0" * 40}
        with tempfile.TemporaryDirectory() as out:
            self.plot.plot(summary, out, diagnosis_dir=Path(out) / "none")
            svg = (Path(out) / "checks-cost.svg").read_text()
        self.assertIn("Rust with Miri", svg)

    def test_a_preliminary_run_says_so_on_every_chart_and_names_the_typescript_runtime(self):
        recs = records("w", "c", [100, 100, 100]) + records("w", "miri", [150, 150, 150]) + records("w", "typescript", [200, 200, 200])
        for record in recs:
            record["machine_profile"] = "preliminary-vps"
        summary = summarize_records(recs)
        summary["provenance"] = {"miri_version": "miri 9.9.9 (test)", "cpu_model": "test cpu", "miri_sha": "0" * 40}
        with tempfile.TemporaryDirectory() as out:
            self.plot.plot(summary, out, diagnosis_dir=Path(out) / "none")
            for name in ("time-vs-c", "memory-vs-c"):
                svg = (Path(out) / f"{name}.svg").read_text()
                self.assertIn("PRELIMINARY", svg)
                self.assertIn("TypeScript (V8 via Deno)", svg)

    def test_an_unreproduced_cell_is_labelled_not_drawn(self):
        miri = next(c for c in self.summary["cells"] if (c["workload"], c["language"], c["mode"]) == ("w", "miri", "cold"))
        miri["reproduced"] = False
        with tempfile.TemporaryDirectory() as out:
            svg = self.draw(out)
        self.assertIn("unreproduced", svg)

    def test_the_lost_table_lists_every_miri_cell_to_diagnose(self):
        with tempfile.TemporaryDirectory() as out:
            self.draw(out)
            table = (Path(out) / "where-miri-lost.md").read_text()
        self.assertIn("| w | S | cold | 1.50 |", table)
        self.assertIn("not yet diagnosed", table)


@unittest.skipUnless(HAS_MATPLOTLIB, "matplotlib is not installed")
class ExportTest(unittest.TestCase):
    def setUp(self):
        import export_site

        self.export_site = export_site
        self.scratch = tempfile.TemporaryDirectory()
        self.run = Path(self.scratch.name) / "run"
        self.run.mkdir()
        (self.run / "summary.json").write_text(json.dumps(fixture()))
        self.site = Path(self.scratch.name) / "site"

    def tearDown(self):
        self.scratch.cleanup()

    def test_a_fresh_export_checks_clean(self):
        self.export_site.export(self.run, self.site, [])
        self.assertEqual(self.export_site.check(self.run, self.site, []), [])

    def test_one_hand_edited_number_is_caught(self):
        self.export_site.export(self.run, self.site, [])
        latest = self.site / "_data" / "bench" / "latest.json"
        data = json.loads(latest.read_text())
        miri = next(c for c in data["cells"] if c["language"] == "miri" and c["workload"] == "w")
        miri["vs_c"]["time"] = 1.2
        latest.write_text(json.dumps(data, indent=1, sort_keys=True) + "\n")
        self.assertEqual(self.export_site.check(self.run, self.site, []), [str(Path("_data/bench/latest.json"))])

    def test_the_data_is_a_subset_named_by_version(self):
        self.export_site.export(self.run, self.site, [])
        data = json.loads((self.site / "_data" / "bench" / "9.9.9.json").read_text())
        self.assertEqual(data["miri_version"], "9.9.9")
        self.assertNotIn("p_high", json.dumps(data))
        self.assertTrue((self.site / "assets" / "bench" / "9.9.9" / "time-vs-c.svg").is_file())
        lost = {(c["workload"], c["mode"]): c for c in data["lost"]}
        self.assertAlmostEqual(lost[("w", "cold")]["time_vs_c"], 1.5)
        self.assertEqual(lost[("w", "cold")]["mechanism"], "not yet diagnosed")

    def test_only_a_public_issue_link_reaches_the_site(self):
        for task, shown in (("https://github.com/miri-lang/miri/issues/1", "https://github.com/miri-lang/miri/issues/1"), ("an internal reference", "")):
            bundle = self.run / "diagnosis" / "w-s"
            bundle.mkdir(parents=True, exist_ok=True)
            (bundle / "DIAGNOSIS.md").write_text(f"Hypothesis: struct values on the heap\nTask: {task}\n")
            lost = self.export_site.site_data(fixture(), self.run)["lost"]
            row = next(c for c in lost if c["workload"] == "w" and c["mode"] == "cold")
            self.assertEqual((row["mechanism"], row["task"]), ("struct values on the heap", shown))

    def test_a_preliminary_vps_run_is_exported_with_its_label(self):
        summary = fixture()
        summary["machine_profile"] = "preliminary-vps"
        summary["label"] = "preliminary"
        data = self.export_site.site_data(summary, self.run)
        self.assertEqual(data["label"], "preliminary")

    def test_a_dev_run_is_never_exported(self):
        summary = fixture()
        summary["machine_profile"] = "dev"
        with self.assertRaises(SystemExit):
            self.export_site.site_data(summary, self.run)


if __name__ == "__main__":
    unittest.main()
