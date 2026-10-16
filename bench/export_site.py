# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Export a publication run to the website, or check that the website's copy
is what an export would write.

    python3 bench/export_site.py [--run bench/results/<name>] [--site ../miri-lang.org]
    python3 bench/export_site.py --check [--site ../miri-lang.org]

Writes, under the website checkout:

- `_data/bench/<miri-version>.json` and `_data/bench/latest.json`: a strict
  subset of the run's `summary.json` (medians, ratios, aggregates and
  provenance), nothing computed here, plus the Miri cells to diagnose with
  the mechanism and task each `DIAGNOSIS.md` names;
- `assets/bench/<miri-version>/*.svg`: the charts `bench/plot.py` draws from
  that summary, with the history chart drawn over every committed publication
  run.

The run defaults to the newest publication run committed under
`bench/results/`; a dev run is never exported. `--check` exports into a
scratch directory and compares it file by file with the website, exiting 1
on any difference, so the site cannot show a number the repository does not.
"""

import argparse
import filecmp
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[1]
BENCH = REPO / "bench"
DEFAULT_SITE = REPO.parent / "miri-lang.org"
CHARTS = ("time-vs-c", "memory-vs-c", "time-memory-scatter", "miri-history")
CELL_FIELDS = ("workload", "language", "variant", "size", "mode", "status", "attempts", "diagnose")
METRICS = ("process_wall_ns", "hot_ns", "max_rss_bytes", "user_ns", "sys_ns")


def published_runs(repo=REPO):
    """Every committed publication run's directory, oldest first."""
    listed = subprocess.run(
        ["git", "ls-files", "bench/results/*/summary.json"], cwd=repo, capture_output=True, text=True, check=True
    ).stdout.split()
    runs = []
    for name in listed:
        summary = json.loads((repo / name).read_text())
        if summary["machine_profile"] == "publication":
            runs.append((summary["run_id"], (repo / name).parent))
    return [path for _, path in sorted(runs)]


def version_of(summary):
    found = re.search(r"\d+\.\d+\.\d+\S*", summary["provenance"].get("miri_version") or "")
    if not found:
        raise SystemExit("the run's provenance names no Miri version")
    return found.group(0)


def site_cell(cell):
    out = {key: cell[key] for key in CELL_FIELDS if key in cell}
    metrics = cell.get("metrics", {})
    out["medians"] = {name: metrics[name]["median"] for name in METRICS if name in metrics}
    net = metrics.get("net_rss_bytes")
    if net:
        out["net_rss_bytes"] = net
    for baseline in ("vs_c", "vs_rust"):
        if cell.get(baseline):
            out[baseline] = {k: (v["value"] if isinstance(v, dict) else v) for k, v in cell[baseline].items()}
    return out


def lost_cells(summary, diagnosis_dir):
    """Every Miri cell past the diagnosis thresholds, with the mechanism its
    DIAGNOSIS.md names and the public issue it links. A task reference that is
    not a public URL stays in the repository: the site links only issues."""
    import plot

    lost = []
    for cell in summary["cells"]:
        if cell["language"] != "miri" or not cell.get("diagnose"):
            continue
        mechanism, task = plot.hypothesis(diagnosis_dir, cell["workload"], cell["size"])
        lost.append(
            {
                "workload": cell["workload"],
                "size": cell["size"],
                "mode": cell["mode"],
                "time_vs_c": plot.ratio_of(cell, ("vs_c", "time")),
                "net_rss_vs_rust": plot.ratio_of(cell, ("vs_rust", "net_rss")),
                "mechanism": mechanism,
                "task": task if task.startswith("https://") else "",
            }
        )
    return lost


def site_data(summary, run_dir):
    """The website's copy of a summary: what the page shows and nothing else."""
    if summary["machine_profile"] != "publication":
        raise SystemExit(f"only a publication run is exported; this one ran on '{summary['machine_profile']}'")
    return {
        "schema_version": summary["schema_version"],
        "charter_version": summary["charter_version"],
        "charter_hash": summary["charter_hash"],
        "run_id": summary["run_id"],
        "machine_profile": summary["machine_profile"],
        "miri_version": version_of(summary),
        "provenance": summary["provenance"],
        "cells": [site_cell(c) for c in summary["cells"]],
        "aggregates": summary["aggregates"],
        "results_path": f"bench/results/{Path(run_dir).name}",
        "lost": lost_cells(summary, Path(run_dir) / "diagnosis"),
    }


def history_of(runs):
    import summarize

    return summarize.history([json.loads((run / "summary.json").read_text()) for run in runs])


def export(run_dir, site, runs):
    """Write the run's data and charts under `site`; return the paths written."""
    import plot

    summary = json.loads((Path(run_dir) / "summary.json").read_text())
    data = site_data(summary, run_dir)
    version = data["miri_version"]
    text = json.dumps(data, indent=1, sort_keys=True) + "\n"
    data_dir = Path(site) / "_data" / "bench"
    data_dir.mkdir(parents=True, exist_ok=True)
    written = []
    for name in (f"{version}.json", "latest.json"):
        (data_dir / name).write_text(text)
        written.append(data_dir / name)
    with tempfile.TemporaryDirectory() as scratch:
        plot.plot(summary, scratch, history=history_of(runs) if runs else None, diagnosis_dir=Path(run_dir) / "diagnosis")
        assets = Path(site) / "assets" / "bench" / version
        assets.mkdir(parents=True, exist_ok=True)
        for chart in CHARTS:
            drawn = Path(scratch) / f"{chart}.svg"
            if drawn.is_file():
                (assets / drawn.name).write_bytes(drawn.read_bytes())
                written.append(assets / drawn.name)
    return written


def check(run_dir, site, runs):
    """Every file an export would write, compared with the site's copy."""
    with tempfile.TemporaryDirectory() as scratch:
        fresh = export(run_dir, scratch, runs)
        drifted = []
        for path in fresh:
            relative = path.relative_to(scratch)
            published = Path(site) / relative
            if not published.is_file() or not filecmp.cmp(path, published, shallow=False):
                drifted.append(str(relative))
        return drifted


def main(argv=None):
    sys.path.insert(0, str(BENCH))
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--run", help="a run directory (default: the newest committed publication run)")
    parser.add_argument("--site", default=str(DEFAULT_SITE), help="the website checkout")
    parser.add_argument("--check", action="store_true", help="compare instead of writing")
    args = parser.parse_args(argv)
    runs = published_runs()
    run_dir = Path(args.run) if args.run else (runs[-1] if runs else None)
    if run_dir is None:
        print("no publication run is committed under bench/results/, so there is nothing to export")
        return 0
    if args.check:
        drifted = check(run_dir, args.site, runs)
        for name in drifted:
            print(f"stale on the website: {name} (run bench/export_site.py)", file=sys.stderr)
        return 1 if drifted else 0
    for path in export(run_dir, args.site, runs):
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
