# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Compare tonight's benchmark summary with the nights before it.

    python3 bench/regress.py <tonight>/summary.json <history-dir> [--out regress.json]

Absolute times on a shared CI runner are noise, so every comparison is of a
cell's ratio to C (time) and to Rust (net memory), taken in the same run. A
cell is *suspect* when its ratio is above the median of the last seven nights
by more than the charter's verdict threshold. A cell suspect tonight and on
the night before is a *regression*, and the workflow opens an issue for it. A
cell that was a regression and no longer is has *recovered*, and its issue is
closed.

`<history-dir>` holds one directory per night, each with its `summary.json`
and the `regress.json` this script wrote that night. Standard library only.
"""

import argparse
import json
import statistics
import sys
from pathlib import Path

import run
import summarize

WINDOW = 7
LANGUAGES = ("miri",)


def cell_key(cell):
    return f"{cell['workload']}/{cell['language']}/{cell['size']}/{cell['mode']}"


def measures(cell):
    """The ratios a cell is judged by, each with its coefficient of variation."""
    out = {}
    time = (cell.get("vs_c") or {}).get("time")
    if time:
        out["time_vs_c"] = (time["value"], time["cv"])
    net = (cell.get("vs_rust") or {}).get("net_rss")
    if net:
        out["net_rss_vs_rust"] = (net, 0.0)
    return out


def nights(history_dir):
    """Every earlier night's summary and verdicts, oldest first."""
    found = []
    for directory in sorted(p for p in Path(history_dir).iterdir() if p.is_dir()):
        summary_path = directory / "summary.json"
        if summary_path.is_file():
            regress_path = directory / "regress.json"
            verdicts = json.loads(regress_path.read_text()) if regress_path.is_file() else {"suspect": []}
            found.append((json.loads(summary_path.read_text()), verdicts))
    return found


def baseline(previous):
    """For each cell and measure, the median ratio of the last nights."""
    values = {}
    for summary, _ in previous[-WINDOW:]:
        for cell in summary["cells"]:
            if cell["language"] not in LANGUAGES:
                continue
            for name, (value, _) in measures(cell).items():
                values.setdefault((cell_key(cell), name), []).append(value)
    return {key: statistics.median(vals) for key, vals in values.items()}


def suspects(tonight, base, charter):
    """Every cell and measure whose ratio rose past the verdict threshold."""
    found = []
    for cell in tonight["cells"]:
        if cell["language"] not in LANGUAGES:
            continue
        for name, (value, cv) in measures(cell).items():
            before = base.get((cell_key(cell), name))
            if before is None or before <= 0:
                continue
            change = value / before
            if summarize.verdict(change, cv, cv, charter) == "slower":
                found.append({"cell": cell_key(cell), "measure": name, "change": change, "baseline": before, "tonight": value})
    return found


def judge(tonight, previous, charter):
    """Tonight's verdicts: what is suspect, what has regressed (suspect two
    nights running) and what has recovered."""
    base = baseline(previous)
    found = suspects(tonight, base, charter)
    last = {(s["cell"], s["measure"]) for s in previous[-1][1]["suspect"]} if previous else set()
    last_regressed = {(s["cell"], s["measure"]) for s in previous[-1][1].get("regressed", [])} if previous else set()
    now = {(s["cell"], s["measure"]) for s in found}
    regressed = [s for s in found if (s["cell"], s["measure"]) in last]
    recovered = [{"cell": c, "measure": m} for c, m in sorted(last_regressed - now)]
    return {"run_id": tonight["run_id"], "suspect": found, "regressed": regressed, "recovered": recovered}


def issue_title(regression, sha):
    workload, language, size, mode = regression["cell"].split("/")
    percent = (regression["change"] - 1) * 100
    return f"perf: {workload} {regression['measure']} regressed {percent:.0f}% since {sha[:10]}"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("summary", help="tonight's summary.json")
    parser.add_argument("history", help="a directory of earlier nights")
    parser.add_argument("--out", help="where to write tonight's verdicts")
    args = parser.parse_args(argv)
    tonight = json.loads(Path(args.summary).read_text())
    previous = nights(args.history)
    verdicts = judge(tonight, previous, run.load_charter())
    text = json.dumps(verdicts, indent=1) + "\n"
    if args.out:
        Path(args.out).write_text(text)
    print(text, end="")
    sha = tonight.get("provenance", {}).get("miri_sha", "unknown")
    for regression in verdicts["regressed"]:
        print(f"REGRESSION {issue_title(regression, sha)}", file=sys.stderr)
    return verdicts


if __name__ == "__main__":
    main()
