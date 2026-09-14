#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Fold a round's records into `summary.json` and judge the claims.

The records under `runs/<round>/` are the raw data; this is the only thing that
reads them, and `summary.json` is the only thing the article quotes. A verdict
is computed here rather than written by hand, so a claim cannot be reported as
held by the person who wanted it to hold.

    report.py --round r1
    report.py --round r1 --out summary.json
"""

import argparse
import json
import statistics
import sys
from pathlib import Path

FIELD = Path(__file__).resolve().parent

CPU_JOBS = ("01-word-frequency", "02-ledger-repair", "03-tracker-extension", "04-data-edges")
GPU_JOB = "05-gpu-heat"
BASELINES = ("python", "rust", "typescript")
PACK = "miri-pack"
BARE = "miri-bare"

# How far above a baseline a result still reads as parity rather than behind.
# It was C1's threshold until C1 was tightened to a lead on 2026-09-14; it now
# labels a result and never decides a verdict, so a tie cannot pass for a win.
PARITY_FACTOR = 1.25


def load_records(round_name, runs_root):
    """Every run record of a round.

    The summary this writes lands in the same directory, so a fold that read
    every JSON file under the round would eat its own output on the second run.
    A record is recognised by its shape rather than by its name.
    """
    directory = runs_root / round_name
    if not directory.is_dir():
        raise SystemExit(f"no records under {directory}")
    records = [decode(path) for path in sorted(directory.rglob("*.json"))]
    return [record for record in records if record is not None]


def decode(path):
    content = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(content, dict) or "job" not in content or "hiddenTests" not in content:
        return None
    return content


def is_green(record):
    tests = record["hiddenTests"]
    return record["outcome"] == "finished" and tests["total"] > 0 and tests["passed"] == tests["total"]


def tokens(record):
    """What a run cost, or nothing when its harness reported no usage.

    A run whose tokens were never observed is not a cheap run. It is excluded
    from every cost median and counted under `costUnobserved`, so a harness
    that stopped reporting usage cannot read as an improvement.
    """
    usage = record["tokens"]
    if not usage.get("observed", True):
        return None
    return usage["in"] + usage["out"]


def cell_of(records, job, arm, model):
    return [r for r in records if r["job"] == job and r["arm"] == arm and r["model"] == model]


def summarize_cell(runs):
    green_runs = [run for run in runs if is_green(run)]
    green = [cost for cost in (tokens(run) for run in green_runs) if cost is not None]
    return {
        "runs": len(runs),
        "green": len(green_runs),
        "finishRate": round(len(green_runs) / len(runs), 3) if runs else 0.0,
        "tokensToGreen": spread(green),
        "wallClockToGreen": spread([run["wallClockSeconds"] for run in green_runs]),
        "invocationsToGreen": spread([run["toolInvocations"] for run in green_runs]),
        "toolchainInvocations": spread([run["toolchainInvocations"] for run in runs]),
        "wallClockSeconds": spread([run["wallClockSeconds"] for run in runs]),
        "costUnobserved": sum(1 for run in green_runs if tokens(run) is None),
        "silentWrongAnswers": sum(1 for run in runs if run["silentWrongAnswer"]),
        "capped": sum(1 for run in runs if run["outcome"] == "capped"),
        "abandoned": sum(1 for run in runs if run["outcome"] == "abandoned"),
    }


def spread(values):
    if not values:
        return None
    return {
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
    }


def median_green(cells, job, arm, model, metric="tokensToGreen"):
    """The median of one to-green metric over a cell's green runs, if it has any."""
    cell = cells.get((job, arm, model))
    return cell[metric]["median"] if cell and cell[metric] else None


def build_cells(records):
    jobs = sorted({record["job"] for record in records})
    arms = sorted({record["arm"] for record in records})
    models = sorted({record["model"] for record in records})
    cells = {}
    for job in jobs:
        for arm in arms:
            for model in models:
                runs = cell_of(records, job, arm, model)
                if runs:
                    cells[(job, arm, model)] = summarize_cell(runs)
    return cells, jobs, models


def judge_lead(cells, models, metric):
    """Whether the pack leads every baseline on one to-green metric, per CPU job.

    Each comparison carries a standing — `beats`, `parity` or `behind` — so a
    round that fails the claim still shows how far it is from holding it.
    """
    detail = []
    for model in models:
        for job in CPU_JOBS:
            pack = median_green(cells, job, PACK, model, metric)
            if pack is None:
                detail.append(note(job, model, False, "miri-pack reached no green run", "behind"))
                continue
            for baseline in BASELINES:
                if (job, baseline, model) not in cells:
                    detail.append(note(job, model, False, f"{baseline} was not run"))
                    continue
                other = median_green(cells, job, baseline, model, metric)
                detail.append(lead_note(job, model, baseline, pack, other))
    return verdict(detail)


def lead_note(job, model, baseline, pack, other):
    if other is None:
        return note(job, model, True, f"{baseline} reached no green run; miri-pack did", "beats")
    return note(job, model, pack < other, f"miri-pack {pack} against {baseline} {other}", standing(pack, other))


def standing(pack, other):
    if pack < other:
        return "beats"
    if pack <= other * PARITY_FACTOR:
        return "parity"
    return "behind"


def judge_wrong_answers(cells, models):
    """C2 — the pack declares a wrong answer no more often than a baseline."""
    detail = []
    for model in models:
        pack = sum(cells[(job, PACK, model)]["silentWrongAnswers"] for job in CPU_JOBS if (job, PACK, model) in cells)
        for baseline in BASELINES:
            other = sum(
                cells[(job, baseline, model)]["silentWrongAnswers"]
                for job in CPU_JOBS
                if (job, baseline, model) in cells
            )
            detail.append(note("cpu jobs", model, pack <= other, f"miri-pack {pack} against {baseline} {other}"))
    return verdict(detail)


def judge_gpu(cells, models):
    """C3 — the GPU job is in reach where a baseline's is not, or is half the cost."""
    detail = []
    for model in models:
        unrun = [name for name in BASELINES if (GPU_JOB, name, model) not in cells]
        if unrun:
            detail.append(note(GPU_JOB, model, False, f"{', '.join(unrun)} not run"))
            continue
        pack = median_green(cells, GPU_JOB, PACK, model)
        others = {name: median_green(cells, GPU_JOB, name, model) for name in BASELINES}
        finished = [name for name, value in others.items() if value is not None]
        if pack is not None and len(finished) < len(BASELINES):
            missing = sorted(set(BASELINES) - set(finished))
            detail.append(note(GPU_JOB, model, True, f"miri-pack finished where {', '.join(missing)} did not"))
            continue
        cheapest = min((value for value in others.values() if value is not None), default=None)
        held = pack is not None and cheapest is not None and pack < cheapest / 2
        detail.append(note(GPU_JOB, model, held, f"miri-pack {pack} against the cheapest baseline {cheapest}"))
    return verdict(detail)


def judge_pack_against_bare(cells, models, jobs):
    """C4 — the pack substitutes for pretraining the model never had."""
    detail = []
    for model in models:
        for job in jobs:
            pack, bare = median_green(cells, job, PACK, model), median_green(cells, job, BARE, model)
            pack_cell, bare_cell = cells.get((job, PACK, model)), cells.get((job, BARE, model))
            if pack_cell is None or bare_cell is None:
                detail.append(note(job, model, False, "a cell of the pair was not run"))
                continue
            cheaper = pack is not None and (bare is None or pack < bare)
            finishes = pack_cell["finishRate"] > bare_cell["finishRate"]
            detail.append(
                note(job, model, cheaper and finishes, f"cost {pack} against {bare}, finish rate {pack_cell['finishRate']} against {bare_cell['finishRate']}")
            )
    return verdict(detail)


def judge_pack_loop(cells, models, jobs):
    """The exit criterion's loop condition — the pack costs no more invocations than bare.

    Measured gross. Both Miri arms run on the same compiler, so a defect falls
    on either arm alike, and netting it out would add a hand-typed number to a
    verdict that needs none.
    """
    detail = []
    for model in models:
        for job in jobs:
            if (job, PACK, model) not in cells or (job, BARE, model) not in cells:
                detail.append(note(job, model, False, "a cell of the pair was not run"))
                continue
            pack = median_green(cells, job, PACK, model, "invocationsToGreen")
            bare = median_green(cells, job, BARE, model, "invocationsToGreen")
            held = pack is not None and (bare is None or pack <= bare)
            detail.append(note(job, model, held, f"invocations to green {pack} against {bare}"))
    return verdict(detail)


def note(job, model, held, reason, rank=None):
    entry = {"job": job, "model": model, "held": bool(held), "reason": reason}
    if rank is not None:
        entry["standing"] = rank
    return entry


def verdict(detail):
    return {"held": all(entry["held"] for entry in detail) and bool(detail), "detail": detail}


def summarize(round_name, records):
    cells, jobs, models = build_cells(records)
    return {
        "schemaVersion": 1,
        "round": round_name,
        "records": len(records),
        "models": models,
        "jobs": jobs,
        "cells": [
            {"job": job, "arm": arm, "model": model, **values}
            for (job, arm, model), values in sorted(cells.items())
        ],
        "claims": {
            "C1": judge_lead(cells, models, "tokensToGreen"),
            "C2": judge_wrong_answers(cells, models),
            "C3": judge_gpu(cells, models),
            "C4": judge_pack_against_bare(cells, models, jobs),
            "C5": judge_lead(cells, models, "wallClockToGreen"),
        },
        "exitCriterion": {
            "packLoop": judge_pack_loop(cells, models, jobs),
        },
    }


def main(argv):
    parser = argparse.ArgumentParser(description="fold a round's records into summary.json")
    parser.add_argument("--round", default="r1")
    parser.add_argument("--out", default=None, help="where to write; the round's summary.json by default")
    parser.add_argument(
        "--runs-root",
        default=None,
        help="where the rounds live; this directory's runs/ by default",
    )
    arguments = parser.parse_args(argv)

    runs_root = Path(arguments.runs_root) if arguments.runs_root else FIELD / "runs"
    records = load_records(arguments.round, runs_root)
    summary = summarize(arguments.round, records)
    destination = Path(arguments.out) if arguments.out else runs_root / arguments.round / "summary.json"
    destination.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

    held = [name for name, claim in summary["claims"].items() if claim["held"]]
    loop = "holds" if summary["exitCriterion"]["packLoop"]["held"] else "fails"
    print(f"{destination}: {len(records)} records, claims held: {', '.join(held) or 'none'}; pack loop {loop}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
