# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Turn the runner's records into a summary, and compare two summaries.

    python3 bench/summarize.py bench/results/<run>/          # writes summary.json and summary.md there
    python3 bench/summarize.py compare <A>/summary.json <B>/summary.json
    python3 bench/summarize.py history <dir-of-runs>/         # one point per run, per cell

Every number is computed by the rules of `bench/CHARTER.md` section 6, and only
here: the charts and the website read `summary.json` and carry no judgement of
their own. Records whose charter hash is not the charter on disk are refused,
and so is a run that mixes machine profiles or charter versions. Standard
library only.
"""

import argparse
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path

import run

SCHEMA_VERSION = 1
# The metrics summarized per cell, by mode.
COLD_METRICS = ("process_wall_ns", "user_ns", "sys_ns", "max_rss_bytes")
HOT_METRICS = ("hot_ns",)
# The metric each mode's time ratio is taken on.
TIME_METRIC = {"cold": "process_wall_ns", "hot": "hot_ns"}
REFERENCE = "c"
MEMORY_REFERENCE = "rust"


# Reading records.


def load_records(run_dir):
    path = Path(run_dir) / "results.jsonl"
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]


def check_provenance(records, expected_hash):
    """Every reason these records may not be summarized together."""
    problems = []
    hashes = {r["charter_hash"] for r in records}
    if hashes - {expected_hash}:
        problems.append("records were made under a charter other than the one on disk")
    for key in ("machine_profile", "charter_version", "run_id"):
        if len({r[key] for r in records}) > 1:
            problems.append(f"records mix more than one {key}")
    return problems


# Statistics, as the charter defines them.


def nearest_rank(values, q):
    """The q-quantile by the nearest-rank method: an observed value."""
    ordered = sorted(values)
    return ordered[max(0, math.ceil(q * len(ordered)) - 1)]


def coefficient_of_variation(values):
    if len(values) < 2 or statistics.mean(values) == 0:
        return 0.0
    return statistics.stdev(values) / statistics.mean(values)


def describe(values, p_high):
    return {
        "n": len(values),
        "median": statistics.median(values),
        "min": min(values),
        "p_high": nearest_rank(values, p_high),
        "mad": statistics.median(abs(v - statistics.median(values)) for v in values),
        "cv": coefficient_of_variation(values),
    }


def geometric_mean(values):
    return math.exp(sum(math.log(v) for v in values) / len(values))


# Cells.


def cell_key(record):
    return (record["workload"], record["language"], record["variant"], record["size"], record["mode"])


def key_dict(key):
    workload, language, variant, size, mode = key
    return {"workload": workload, "language": language, "variant": variant, "size": size, "mode": mode}


def group_cells(records):
    """Measured runs of each cell's last attempt, and each unmeasured cell's
    status. Runs of earlier attempts are published but not summarized."""
    runs = defaultdict(list)
    statuses = {}
    for record in records:
        if record["mode"] == "alloc-count":
            continue
        key = cell_key(record)
        if record.get("run_index") is None:
            statuses[key] = record["status"]
        else:
            runs[key].append(record)
    last = {}
    for key, cell_runs in runs.items():
        final = max(r["attempt"] for r in cell_runs)
        last[key] = [r for r in cell_runs if r["attempt"] == final]
    return last, statuses


def summarize_cell(cell_runs, charter):
    """Statistics of one cell's runs, and whether it is noisy or timed out."""
    p_high = charter["statistics"]["p_high"]
    ok = [r for r in cell_runs if r["status"] == "ok"]
    if len(ok) < len(cell_runs):
        return {"status": "timeout", "attempts": max(r["attempt"] for r in cell_runs) + 1}
    mode = cell_runs[0]["mode"]
    metrics = {}
    for metric in COLD_METRICS if mode == "cold" else HOT_METRICS:
        values = [r[metric] for r in ok if r.get(metric) is not None]
        if values:
            metrics[metric] = describe(values, p_high)
    if mode == "cold":
        metrics["net_rss_bytes"] = net_rss(metrics, ok)
    threshold = charter["statistics"]["noisy_cv"]
    noisy = any(m["cv"] > threshold for name, m in metrics.items() if name in ("process_wall_ns", "hot_ns", "max_rss_bytes"))
    return {
        "status": "noisy" if noisy else "ok",
        "attempts": cell_runs[0]["attempt"] + 1,
        "metrics": metrics,
        "binary_bytes": cell_runs[0].get("binary_bytes"),
        "floor_rss_bytes": cell_runs[0].get("floor_rss_bytes"),
        "startup_ns": cell_runs[0].get("startup_ns"),
    }


def net_rss(metrics, runs):
    """Peak memory above the runtime floor; at or below the floor it is not a
    number, because peak sizes do not subtract exactly."""
    floor = runs[0].get("floor_rss_bytes")
    if floor is None or "max_rss_bytes" not in metrics:
        return {"value": None, "at_or_below_floor": False}
    value = metrics["max_rss_bytes"]["median"] - floor
    return {"value": value if value > 0 else None, "at_or_below_floor": value <= 0}


# Ratios.


def ratio(cells, key, reference, metric):
    """The cell's median over the reference language's, same run, workload,
    variant, size and mode, with the CV of the ratio; None without both."""
    workload, _, variant, size, mode = key
    ref = cells.get((workload, reference, variant, size, mode))
    mine = cells.get(key)
    if not ref or not mine or "metrics" not in ref or "metrics" not in mine:
        return None
    a, b = mine["metrics"].get(metric), ref["metrics"].get(metric)
    if a is None or b is None or b["median"] == 0:
        return None
    return {"value": a["median"] / b["median"], "cv": math.hypot(a["cv"], b["cv"])}


def net_ratio(cells, key, reference):
    workload, _, variant, size, mode = key
    ref = cells.get((workload, reference, variant, size, mode))
    mine = cells.get(key)
    if not ref or not mine or "metrics" not in ref or "metrics" not in mine:
        return None
    a = mine["metrics"].get("net_rss_bytes", {}).get("value")
    b = ref["metrics"].get("net_rss_bytes", {}).get("value")
    return a / b if a and b else None


def add_ratios(cells, charter):
    for key, cell in cells.items():
        if "metrics" not in cell:
            continue
        mode = key[4]
        cell["vs_c"] = {"time": ratio(cells, key, REFERENCE, TIME_METRIC[mode])}
        cell["vs_miri"] = {"time": ratio(cells, key, "miri", TIME_METRIC[mode])}
        if mode == "cold":
            cell["vs_c"]["max_rss"] = ratio(cells, key, REFERENCE, "max_rss_bytes")
            cell["vs_rust"] = {"max_rss": ratio(cells, key, MEMORY_REFERENCE, "max_rss_bytes"), "net_rss": net_ratio(cells, key, MEMORY_REFERENCE)}
        cell["diagnose"] = needs_diagnosis(key, cell, charter)


def needs_diagnosis(key, cell, charter):
    """A Miri cell past the charter's loss thresholds."""
    if key[1] != "miri":
        return False
    time = (cell["vs_c"].get("time") or {}).get("value")
    memory = cell.get("vs_rust", {}).get("net_rss")
    thresholds = charter["diagnosis"]
    return bool((time and time >= thresholds["time_vs_c"]) or (memory and memory >= thresholds["net_rss_vs_rust"]))


def aggregates(cells, manifests):
    """Geometric means of each language's time ratio to C, over every workload
    with a ratio, overall and per feature tag, with the workloads counted."""
    groups = defaultdict(list)
    for key, cell in cells.items():
        value = ((cell.get("vs_c") or {}).get("time") or {}).get("value")
        if value is None or value <= 0:
            continue
        workload, language, variant, size, mode = key
        groups[(language, variant, size, mode, "all")].append(value)
        for tag in manifests.get(workload, {}).get("tags", []):
            groups[(language, variant, size, mode, tag)].append(value)
    out = []
    for (language, variant, size, mode, tag), values in sorted(groups.items()):
        out.append(
            {
                "language": language,
                "variant": variant,
                "size": size,
                "mode": mode,
                "tag": tag,
                "geomean_vs_c": geometric_mean(values),
                "coverage": len(values),
            }
        )
    return out


# The summary.


def summarize(records, charter, expected_hash, manifests, env=None):
    if problems := check_provenance(records, expected_hash):
        raise SystemExit("refusing to summarize:\n  " + "\n  ".join(problems))
    last, statuses = group_cells(records)
    cells = {key: summarize_cell(runs, charter) for key, runs in last.items()}
    for key, status in statuses.items():
        cells.setdefault(key, {"status": status})
    add_ratios(cells, charter)
    first = records[0]
    return {
        "schema_version": SCHEMA_VERSION,
        "charter_version": first["charter_version"],
        "charter_hash": first["charter_hash"],
        "run_id": first["run_id"],
        "machine_profile": first["machine_profile"],
        "provenance": env or {},
        "cells": [key_dict(key) | cell for key, cell in sorted(cells.items())],
        "aggregates": aggregates(cells, manifests),
    }


def workload_manifests():
    return {w.name: w.manifest for w in run.load_workloads(None)}


# Comparing two summaries.


def comparable(a, b):
    if a["machine_profile"] != b["machine_profile"]:
        return "the two runs were made on different machine profiles"
    if a["charter_version"] != b["charter_version"]:
        return "the two runs were made under different charter versions"
    return None


def verdict(ratio_b_over_a, cv_a, cv_b, charter):
    """faster, same or slower, by the charter's noise-aware threshold."""
    threshold = max(charter["verdict"]["min_threshold"], charter["verdict"]["cv_multiplier"] * max(cv_a, cv_b))
    if ratio_b_over_a < 1 - threshold:
        return "faster"
    if ratio_b_over_a > 1 + threshold:
        return "slower"
    return "same"


def compare(a, b, charter):
    if (problem := comparable(a, b)) is not None:
        raise SystemExit(f"refusing to compare: {problem}")
    index = {cell_identity(c): c for c in a["cells"]}
    rows = []
    for cell in b["cells"]:
        before = index.get(cell_identity(cell))
        time_a = ((before or {}).get("vs_c") or {}).get("time")
        time_b = (cell.get("vs_c") or {}).get("time")
        if not time_a or not time_b:
            continue
        r = time_b["value"] / time_a["value"]
        rows.append(cell_identity_dict(cell) | {"ratio": r, "verdict": verdict(r, time_a["cv"], time_b["cv"], charter)})
    rows.sort(key=lambda row: row["ratio"])
    return {"schema_version": SCHEMA_VERSION, "a": a["run_id"], "b": b["run_id"], "rows": rows}


def cell_identity(cell):
    return (cell["workload"], cell["language"], cell["variant"], cell["size"], cell["mode"])


def cell_identity_dict(cell):
    return {k: cell[k] for k in ("workload", "language", "variant", "size", "mode")}


def history(summaries):
    """Each cell's time ratio to C over runs, in run order, for the version
    chart; one profile only."""
    profiles = {s["machine_profile"] for s in summaries}
    if len(profiles) > 1:
        raise SystemExit("refusing a history across machine profiles")
    series = defaultdict(list)
    for summary in sorted(summaries, key=lambda s: s["run_id"]):
        version = summary["provenance"].get("miri_version")
        for cell in summary["cells"]:
            time = (cell.get("vs_c") or {}).get("time")
            if time:
                series[cell_identity(cell)].append({"run_id": summary["run_id"], "miri_version": version, "vs_c": time["value"]})
    return [{**dict(zip(("workload", "language", "variant", "size", "mode"), key)), "points": points} for key, points in sorted(series.items())]


# Markdown.


def fmt(value, digits=2):
    return "—" if value is None else f"{value:.{digits}f}"


def markdown(summary):
    lines = [
        f"# Benchmark summary {summary['run_id']}",
        "",
        f"Profile `{summary['machine_profile']}`, charter version {summary['charter_version']}. Generated by `bench/summarize.py`; do not edit.",
        "",
        "| Workload | Language | Size | Mode | Status | Median time (ms) | × C | Max RSS (MB) |",
        "|---|---|---|---|---|---|---|---|",
    ]
    for cell in summary["cells"]:
        metrics = cell.get("metrics", {})
        time = metrics.get(TIME_METRIC.get(cell["mode"], ""), {}).get("median")
        rss = metrics.get("max_rss_bytes", {}).get("median")
        vs_c = ((cell.get("vs_c") or {}).get("time") or {}).get("value")
        lines.append(
            f"| {cell['workload']} | {cell['language']} | {cell['size']} | {cell['mode']} | {cell['status']}"
            f" | {fmt(time / 1e6 if time else None, 1)} | {fmt(vs_c)} | {fmt(rss / 2**20 if rss else None, 1)} |"
        )
    lines += ["", "| Language | Size | Mode | Tag | Geomean × C | Workloads |", "|---|---|---|---|---|---|"]
    for row in summary["aggregates"]:
        lines.append(f"| {row['language']} | {row['size']} | {row['mode']} | {row['tag']} | {fmt(row['geomean_vs_c'])} | {row['coverage']} |")
    return "\n".join(lines) + "\n"


# Command line.


def write_summary(run_dir):
    run_dir = Path(run_dir)
    env_path = run_dir / "env.json"
    env = json.loads(env_path.read_text()) if env_path.is_file() else {}
    summary = summarize(load_records(run_dir), run.load_charter(), run.charter_hash(), workload_manifests(), env)
    (run_dir / "summary.json").write_text(json.dumps(summary, indent=1, sort_keys=True) + "\n")
    (run_dir / "summary.md").write_text(markdown(summary))
    return summary


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] == ["compare"]:
        a, b = (json.loads(Path(p).read_text()) for p in argv[1:3])
        print(json.dumps(compare(a, b, run.load_charter()), indent=1))
    elif argv[:1] == ["history"]:
        summaries = [json.loads(p.read_text()) for p in sorted(Path(argv[1]).glob("*/summary.json"))]
        print(json.dumps(history(summaries), indent=1))
    else:
        parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
        parser.add_argument("run_dir", help="a results directory bench/run.py wrote")
        summary = write_summary(parser.parse_args(argv).run_dir)
        print(f"summarized {len(summary['cells'])} cells")


if __name__ == "__main__":
    main()
