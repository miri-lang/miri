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
# How a language is named in a table or chart where its id alone would
# mislead: the TypeScript arm measures V8, since Deno strips the annotations.
LANGUAGE_LABELS = {"typescript": "TypeScript (V8 via Deno)"}
# Rust built with Miri's run-time checks: Miri's distance from it is the
# compiler's, and Rust's is the price of the checks (charter section 2).
CHECKED_REFERENCE = "rust-checked"


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
        if record["mode"] in ("alloc-count", "compile"):
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
    cell = {
        "status": "noisy" if noisy else "ok",
        "attempts": cell_runs[0]["attempt"] + 1,
        "metrics": metrics,
        "binary_bytes": cell_runs[0].get("binary_bytes"),
        "floor_rss_bytes": cell_runs[0].get("floor_rss_bytes"),
        "startup_ns": cell_runs[0].get("startup_ns"),
    }
    if (share := steal_share(ok)) is not None:
        cell["steal_share"] = share
    return cell


def steal_share(runs):
    """The share of the runs' CPU time the hypervisor took from the pinned
    core, or None where the runner could not read steal time."""
    measured = [r for r in runs if r.get("steal_ns") is not None]
    cpu = sum(r["user_ns"] + r["sys_ns"] for r in measured)
    return sum(r["steal_ns"] for r in measured) / cpu if measured and cpu else None


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
        cell["vs_rust_checked"] = {"time": ratio(cells, key, CHECKED_REFERENCE, TIME_METRIC[mode])}
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
    """Geometric means of each language's time ratio to C, and of its peak
    memory ratio to C, over every workload with a ratio, overall and per
    feature tag, with the workloads each covers."""
    groups = defaultdict(lambda: {"time": [], "memory": []})
    for key, cell in cells.items():
        workload, language, variant, size, mode = key
        vs_c = cell.get("vs_c") or {}
        time = (vs_c.get("time") or {}).get("value")
        memory = (vs_c.get("max_rss") or {}).get("value")
        for tag in ["all", *manifests.get(workload, {}).get("tags", [])]:
            group = groups[(language, variant, size, mode, tag)]
            if time and time > 0:
                group["time"].append(time)
            if memory and memory > 0:
                group["memory"].append(memory)
    out = []
    for (language, variant, size, mode, tag), group in sorted(groups.items()):
        if not group["time"]:
            continue
        row = {
            "language": language,
            "variant": variant,
            "size": size,
            "mode": mode,
            "tag": tag,
            "geomean_vs_c": geometric_mean(group["time"]),
            "coverage": len(group["time"]),
        }
        if group["memory"]:
            row["geomean_max_rss_vs_c"] = geometric_mean(group["memory"])
            row["memory_coverage"] = len(group["memory"])
        out.append(row)
    return out


# The summary.


def summarize(records, charter, expected_hash, manifests, env=None, reproduces=None):
    if problems := check_provenance(records, expected_hash):
        raise SystemExit("refusing to summarize:\n  " + "\n  ".join(problems))
    last, statuses = group_cells(records)
    cells = {key: summarize_cell(runs, charter) for key, runs in last.items()}
    for key, status in statuses.items():
        cells.setdefault(key, {"status": status})
    add_ratios(cells, charter)
    first = records[0]
    summary = {
        "schema_version": SCHEMA_VERSION,
        "charter_version": first["charter_version"],
        "charter_hash": first["charter_hash"],
        "run_id": first["run_id"],
        "machine_profile": first["machine_profile"],
        "provenance": env or {},
        "cells": [key_dict(key) | cell for key, cell in sorted(cells.items())],
        "aggregates": aggregates(cells, manifests),
        "port_notes": port_notes(cells, manifests),
        "explanations": explanations(cells, manifests),
        "compile": compile_times(records),
        "coverage": coverage(records),
        "label": profile_label(first["machine_profile"]),
        "max_steal_share": max((c["steal_share"] for c in cells.values() if "steal_share" in c), default=None),
    }
    if reproduces is not None:
        mark_reproduction(summary, reproduces, charter)
    return summary


def coverage(records):
    """Per language, the workloads it was measured on and every cell it was
    not, with the status and the reason the runner recorded: a released
    compiler that cannot build a port is listed here, never dropped."""
    measured = defaultdict(set)
    unmeasured = defaultdict(list)
    for record in records:
        if record["mode"] in ("alloc-count", "compile"):
            continue
        language = record["language"]
        if record.get("run_index") is not None:
            measured[language].add(record["workload"])
        else:
            entry = {key: record[key] for key in ("workload", "size", "mode", "status")}
            unmeasured[language].append(entry | ({"reason": record["reason"]} if record.get("reason") else {}))
    return {
        language: {
            "measured": sorted(measured[language]),
            "unmeasured": sorted(unmeasured[language], key=lambda e: (e["workload"], e["size"], e["mode"])),
        }
        for language in sorted(measured.keys() | unmeasured.keys())
    }


def compile_times(records):
    """Per workload and optimisation level, the best of the Miri port's timed
    builds: the compiler's wall time and its peak resident memory. Recorded
    for awareness; nothing is gated on it."""
    builds = defaultdict(list)
    for record in records:
        if record["mode"] == "compile" and record["status"] == "ok":
            builds[(record["workload"], f"opt{record['opt_level']}")].append(record)
    out = {}
    for (workload, level), runs in sorted(builds.items()):
        out.setdefault(workload, {})[level] = {
            "wall_ns": min(r["process_wall_ns"] for r in runs),
            "max_rss_bytes": min(r["max_rss_bytes"] for r in runs),
            "runs": len(runs),
        }
    return out


def profile_label(profile_name):
    """The label a publishable profile's charts carry, such as "preliminary"."""
    path = run.BENCH / "machine" / f"{profile_name}.toml"
    if not path.is_file():
        return None
    profile = run.load_profile(profile_name)
    return profile.get("label") if profile.get("publishable") else None


def mark_reproduction(summary, first, charter):
    """Mark each cell of a second session reproduced when its ratio to C agrees
    with the first session's under the charter's verdict rule, and record how
    far the two sessions drifted apart."""
    rows = {cell_identity(row): row for row in compare(first, summary, charter)["rows"]}
    for cell in summary["cells"]:
        if (row := rows.get(cell_identity(cell))) is not None:
            cell["reproduced"] = row["verdict"] == "same"
    verdicts = [row["verdict"] for row in rows.values()]
    summary["reproduction"] = {
        "of": first["run_id"],
        "agreeing": verdicts.count("same"),
        "disagreeing": len(verdicts) - verdicts.count("same"),
        "max_drift": max((abs(row["ratio"] - 1) for row in rows.values()), default=0.0),
    }


def port_notes(cells, manifests):
    """Each measured workload's manifest `notes`: what a reader needs beside its
    ratios to read them fairly, such as a reference port's known handicap."""
    measured = sorted({key[0] for key in cells})
    return {w: list(manifests[w]["notes"]) for w in measured if manifests.get(w, {}).get("notes")}


def explanations(cells, manifests):
    """Each measured cell's reason for where its language lands, from its
    workload manifest's `explain` table, for the languages the run measured."""
    measured = {}
    for workload, language, *_ in cells:
        reason = manifests.get(workload, {}).get("explain", {}).get(language)
        if reason:
            measured.setdefault(workload, {})[language] = reason
    return {w: dict(sorted(reasons.items())) for w, reasons in sorted(measured.items())}


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


def history_order(summary):
    """Where a run stands on the version line: by the commit time of the
    compiler it measured when the run recorded one (bench/history.py does),
    else by when it ran."""
    return (summary["provenance"].get("commit_time") or "", summary["run_id"])


def history_point(index, summary):
    """What every point of one run carries: its place, its label (the release
    tag, or the tree it measured) and whether its compiler's optimisation
    level reached the code generator, as the run recorded it."""
    provenance = summary["provenance"]
    version = provenance.get("miri_version")
    return {
        "index": index,
        "run_id": summary["run_id"],
        "miri_version": version,
        "label": provenance.get("point_label") or version or summary["run_id"],
        "codegen_optimized": provenance.get("codegen_optimized"),
    }


def coverage_counts(summary):
    """Per language, how many workloads were measured and how many were
    unsupported in any cell."""
    return {
        language: {
            "measured": len(seen["measured"]),
            "unsupported": len({e["workload"] for e in seen["unmeasured"] if e["status"] == "unsupported"}),
        }
        for language, seen in summary.get("coverage", {}).items()
    }


def history(summaries):
    """Each cell's time ratio to C over runs, in version order, for the
    version chart; each language's geometric mean over all workloads as a
    series marked `aggregate: geomean` whose points carry the workloads they
    cover; and one point per run in a series marked `aggregate: runs`, with
    how many workloads each language was measured on and how many it could
    not build, so a run with no ratio still has its place on the line. One
    profile only."""
    profiles = {s["machine_profile"] for s in summaries}
    if len(profiles) > 1:
        raise SystemExit("refusing a history across machine profiles")
    series = defaultdict(list)
    for index, summary in enumerate(sorted(summaries, key=history_order)):
        point = history_point(index, summary)
        series[("runs", None, None, None, None, None)].append(point | {"coverage": coverage_counts(summary)})
        for cell in summary["cells"]:
            time = (cell.get("vs_c") or {}).get("time")
            if time:
                series[("", *cell_identity(cell))].append(point | {"vs_c": time["value"]})
        for row in summary["aggregates"]:
            if row["tag"] == "all":
                key = ("geomean", None, row["language"], row["variant"], row["size"], row["mode"])
                series[key].append(point | {"vs_c": row["geomean_vs_c"], "coverage": row["coverage"]})
    names = ("aggregate", "workload", "language", "variant", "size", "mode")
    return [
        {key: value for key, value in zip(names, identity) if key != "aggregate" or value} | {"points": points}
        for identity, points in sorted(series.items(), key=lambda item: tuple(str(part) for part in item[0]))
    ]


# Markdown.


def language_label(language):
    return LANGUAGE_LABELS.get(language, language)


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
            f"| {cell['workload']} | {language_label(cell['language'])} | {cell['size']} | {cell['mode']} | {cell['status']}"
            f" | {fmt(time / 1e6 if time else None, 1)} | {fmt(vs_c)} | {fmt(rss / 2**20 if rss else None, 1)} |"
        )
    lines += ["", "| Language | Size | Mode | Tag | Geomean × C | Workloads |", "|---|---|---|---|---|---|"]
    for row in summary["aggregates"]:
        lines.append(f"| {language_label(row['language'])} | {row['size']} | {row['mode']} | {row['tag']} | {fmt(row['geomean_vs_c'])} | {row['coverage']} |")
    if compiled := summary.get("compile"):
        lines += ["", "## Compile time", "", "Best of the Miri port's timed builds. Recorded for awareness; nothing is gated on it.", ""]
        lines += ["| Workload | Level | Wall (ms) | Peak RSS (MB) |", "|---|---|---|---|"]
        for workload, levels in compiled.items():
            for level, best in levels.items():
                lines.append(f"| {workload} | {level} | {fmt(best['wall_ns'] / 1e6, 1)} | {fmt(best['max_rss_bytes'] / 2**20, 1)} |")
    if unmeasured := [(language, entry) for language, seen in summary.get("coverage", {}).items() for entry in seen["unmeasured"]]:
        lines += ["", "## Not measured", "", "| Workload | Language | Size | Mode | Status | Reason |", "|---|---|---|---|---|---|"]
        for language, entry in unmeasured:
            reason = entry.get("reason", "").replace("|", "\\|")
            lines.append(f"| {entry['workload']} | {language_label(language)} | {entry['size']} | {entry['mode']} | {entry['status']} | {reason} |")
    if reasons := summary.get("explanations"):
        lines += ["", "## Why each language lands where it does", ""]
        lines += [f"- **{workload}**, {language}: {reason}" for workload, by_language in reasons.items() for language, reason in by_language.items()]
    if notes := summary.get("port_notes"):
        lines += ["", "## Port notes", ""]
        lines += [f"- **{workload}**: {note}" for workload, items in notes.items() for note in items]
    return "\n".join(lines) + "\n"


# Command line.


def write_summary(run_dir, reproduces=None):
    run_dir = Path(run_dir)
    env_path = run_dir / "env.json"
    env = json.loads(env_path.read_text()) if env_path.is_file() else {}
    first = json.loads(Path(reproduces).read_text()) if reproduces else None
    summary = summarize(load_records(run_dir), run.load_charter(), run.charter_hash(), workload_manifests(), env, first)
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
        parser.add_argument("--reproduces", help="the summary.json of the first session this run reproduces")
        args = parser.parse_args(argv)
        summary = write_summary(args.run_dir, args.reproduces)
        print(f"summarized {len(summary['cells'])} cells")


if __name__ == "__main__":
    main()
