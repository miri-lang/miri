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
import tomllib
from pathlib import Path

FIELD = Path(__file__).resolve().parent

# The transcript reader lives beside this script, whichever directory it is run from.
sys.path.insert(0, str(FIELD))
import usage  # noqa: E402
REPO = FIELD.parent.parent

BASELINES = ("python", "rust")
# The Miri arm the claims are about, and the onboarding arms it is compared with.
SKILL = "miri-skill"
GRAMMAR = "miri-grammar"
BOTH = "miri-both"

# How far above the cheaper (or faster) baseline the skill arm may land and
# still hold C1 or C5.
BAR_FACTOR = 1.5

# How far above a baseline a result still reads as parity rather than behind.
# It was C1's threshold until C1 was tightened to a lead on 2026-09-14; it now
# labels a result and never decides a verdict, so a tie cannot pass for a win.
PARITY_FACTOR = 1.25

# The `kind` `bench.py` stamps on an opinion-probe record.
PROBE_KIND = "probe"

# Where the surfaces the published page recommends are listed. The probe's
# ratings are joined against it, which is what turns "what does the page
# recommend" from a reading of its prose into a lookup.
SURFACES = REPO / "skills" / "surfaces.toml"

# The score at or below which a surface fails the probe's condition.
LOW_RATING = 2

def miri_arms():
    """The arms that run on the compiler under test, as `arms.toml` declares them."""
    with open(FIELD / "arms.toml", "rb") as handle:
        return tuple(arm["id"] for arm in tomllib.load(handle)["arm"] if arm["language"] == "Miri")


# The arms that run on the compiler under test.
MIRI_ARMS = miri_arms()


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
    """A run record, or nothing for a file that is not one.

    An opinion-probe record is refused rather than skipped. Its ratings are what
    a subject said about itself, and one found inside a round means the round's
    data was mixed by hand: folding past it quietly would hide that.
    """
    content = json.loads(path.read_text(encoding="utf-8"))
    if isinstance(content, dict) and content.get("kind") == PROBE_KIND:
        raise SystemExit(
            f"{path} is an opinion-probe record; probes are never folded into a round, "
            "and their records belong under runs/<round>.probe/"
        )
    if not isinstance(content, dict) or "job" not in content or "hiddenTests" not in content:
        return None
    if not (content.get("isolation") or {}).get("clean"):
        raise SystemExit(
            f"{path} records a run that was not isolated from the operator's configuration "
            f"({content.get('isolation')!r}); a hook, MCP server or plugin the benchmark did "
            "not ship belongs to no arm, so the run is re-run rather than folded"
        )
    return content


def load_probe_records(round_name, runs_root):
    """Every opinion-probe record beside a round.

    A probe lives in `runs/<round>.probe/`, never inside the round. An absent
    directory is not an error and not an empty result either: it is a round
    whose opinion condition nobody measured, and a verdict computed from no
    records does not hold.
    """
    directory = runs_root / f"{round_name}.{PROBE_KIND}"
    if not directory.is_dir():
        return []
    records = [decode_probe(path) for path in sorted(directory.rglob("*.json"))]
    return [record for record in records if record is not None]


def decode_probe(path):
    """A probe record, or nothing for a file that is not one.

    A measured run record found here is refused for the same reason a probe
    record inside a round is: it means the two were mixed by hand, and the one
    place that reads them must say so rather than quietly pick a side.
    """
    content = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(content, dict):
        return None
    if content.get("kind") == PROBE_KIND:
        return content
    if "hiddenTests" in content:
        raise SystemExit(
            f"{path} is a measured run record kept beside a probe; a round's runs belong "
            "under runs/<round>/ and are never folded into the opinion condition"
        )
    return None


def recommended_surfaces():
    """The surfaces the published page recommends, commands before documents."""
    with open(SURFACES, "rb") as handle:
        listed = tomllib.load(handle)
    return listed["commands"] + listed["documents"]


def job_tracks():
    """The CPU jobs and the GPU jobs, as each job's `job.toml` declares them."""
    cpu, gpu = [], []
    for path in sorted((FIELD / "jobs").glob("*/job.toml")):
        with open(path, "rb") as handle:
            (gpu if tomllib.load(handle).get("gpu", False) else cpu).append(path.parent.name)
    return tuple(cpu), tuple(gpu)


CPU_JOBS, GPU_JOBS = job_tracks()


def is_green(record):
    """Every hidden test passed, and a GPU job's kernel is in the source."""
    tests = record["hiddenTests"]
    passed = record["outcome"] == "finished" and tests["total"] > 0 and tests["passed"] == tests["total"]
    return passed and not tests.get("notOnGpu", False)


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


def judge_within_bar(cells, models, metric):
    """Whether the skill arm lands within the bar of the better baseline, per GPU job.

    The verdict compares against the cheaper (or faster) of the baselines that
    reached green. Each baseline is also reported with a standing — `beats`,
    `parity` or `behind` — so a round shows how far it is from a lead.
    """
    detail = []
    for model in models:
        for job in GPU_JOBS:
            detail.extend(bar_notes(cells, job, model, metric))
    return verdict(detail)


def bar_notes(cells, job, model, metric):
    skill = median_green(cells, job, SKILL, model, metric)
    if skill is None:
        return [note(job, model, False, f"{SKILL} reached no green run", "behind")]
    unrun = [name for name in BASELINES if (job, name, model) not in cells]
    if unrun:
        return [note(job, model, False, f"{', '.join(unrun)} not run")]
    others = {name: median_green(cells, job, name, model, metric) for name in BASELINES}
    standings = [standing_note(job, model, name, skill, value) for name, value in others.items()]
    best = min((value for value in others.values() if value is not None), default=None)
    if best is None:
        return [note(job, model, True, f"no baseline reached green; {SKILL} did", "beats"), *standings]
    held = skill <= best * BAR_FACTOR
    return [note(job, model, held, f"{SKILL} {skill} against the better baseline {best}"), *standings]


def standing_note(job, model, baseline, skill, other):
    """A label beside the verdict: where the skill arm stands against one baseline."""
    if other is None:
        return note(job, model, True, f"{baseline} reached no green run; {SKILL} did", "beats")
    return note(job, model, True, f"{SKILL} {skill} against {baseline} {other}", standing(skill, other))


def standing(pack, other):
    if pack < other:
        return "beats"
    if pack <= other * PARITY_FACTOR:
        return "parity"
    return "behind"


def judge_wrong_answers(cells, models):
    """C2 — the skill arm declares a wrong answer no more often than a baseline."""
    detail = []
    for model in models:
        skill = sum(cells[(job, SKILL, model)]["silentWrongAnswers"] for job in GPU_JOBS if (job, SKILL, model) in cells)
        for baseline in BASELINES:
            other = sum(
                cells[(job, baseline, model)]["silentWrongAnswers"]
                for job in GPU_JOBS
                if (job, baseline, model) in cells
            )
            detail.append(note("gpu jobs", model, skill <= other, f"{SKILL} {skill} against {baseline} {other}"))
    return verdict(detail)


def judge_gpu(cells, models):
    """C3 — each GPU job is in reach where a baseline's is not, or is half the cost."""
    detail = [gpu_note(cells, job, model) for model in models for job in GPU_JOBS]
    return verdict(detail)


def gpu_note(cells, job, model):
    unrun = [name for name in BASELINES if (job, name, model) not in cells]
    if unrun:
        return note(job, model, False, f"{', '.join(unrun)} not run")
    skill = median_green(cells, job, SKILL, model)
    others = {name: median_green(cells, job, name, model) for name in BASELINES}
    finished = [name for name, value in others.items() if value is not None]
    if skill is not None and len(finished) < len(BASELINES):
        missing = sorted(set(BASELINES) - set(finished))
        return note(job, model, True, f"{SKILL} finished where {', '.join(missing)} did not")
    cheapest = min((value for value in others.values() if value is not None), default=None)
    held = skill is not None and cheapest is not None and skill < cheapest / 2
    return note(job, model, held, f"{SKILL} {skill} against the cheapest baseline {cheapest}")


def judge_skill_against_grammar(cells, models, jobs):
    """C4 — teaching from the skill pack beats teaching from the grammar alone.

    The arm taught from both is reported beside it as a label: whether adding
    the grammar to the pack moved anything is worth seeing, and is not claimed.
    """
    detail = []
    for model in models:
        for job in ran_gpu_jobs(jobs):
            skill, grammar = median_green(cells, job, SKILL, model), median_green(cells, job, GRAMMAR, model)
            skill_cell, grammar_cell = cells.get((job, SKILL, model)), cells.get((job, GRAMMAR, model))
            if skill_cell is None or grammar_cell is None:
                detail.append(note(job, model, False, "a cell of the pair was not run"))
                continue
            cheaper = skill is not None and (grammar is None or skill < grammar)
            # No lower, rather than higher: an arm that finishes every run
            # leaves no rate to beat, and the claim must stay reachable.
            finishes = skill_cell["finishRate"] >= grammar_cell["finishRate"]
            detail.append(
                note(job, model, cheaper and finishes, f"cost {skill} against {grammar}, finish rate {skill_cell['finishRate']} against {grammar_cell['finishRate']}")
            )
            both = median_green(cells, job, BOTH, model)
            if skill is not None and both is not None:
                detail.append(note(job, model, True, f"{BOTH} {both} against {SKILL} {skill}", standing(both, skill)))
    return verdict(detail)


def ran_gpu_jobs(jobs):
    """The GPU jobs a round ran, which the pairwise Miri comparisons are judged over."""
    return [job for job in GPU_JOBS if job in jobs]


def judge_pack_loop(cells, models, jobs):
    """The exit criterion's loop condition — the arm taught from the pack needs no
    more invocations than the arm taught from the grammar.

    Measured gross. Both arms run on the same compiler, so a defect falls on
    either alike, and netting it out would add a hand-typed number to a verdict
    that needs none.
    """
    detail = []
    for model in models:
        for job in ran_gpu_jobs(jobs):
            if (job, SKILL, model) not in cells or (job, GRAMMAR, model) not in cells:
                detail.append(note(job, model, False, "a cell of the pair was not run"))
                continue
            skill = median_green(cells, job, SKILL, model, "invocationsToGreen")
            grammar = median_green(cells, job, GRAMMAR, model, "invocationsToGreen")
            held = skill is not None and (grammar is None or skill <= grammar)
            detail.append(note(job, model, held, f"invocations to green {skill} against {grammar}"))
    return verdict(detail)


def judge_silent_wrong_answers(cells, models, jobs):
    """The exit criterion's first condition — no Miri run answered wrongly in silence.

    Judged on what a record carries rather than on whether the defect behind a
    wrong answer was new: newness is a fact about the compiler's history, not
    about the round, and a condition needing that history could only be typed by
    hand. A subject that declared itself finished on an answer the hidden tests
    refuse restarts the loop whichever Miri arm produced it.
    """
    detail = []
    for model in models:
        for job in jobs:
            for arm in MIRI_ARMS:
                if not any(key[1] == arm for key in cells):
                    continue
                cell = cells.get((job, arm, model))
                if cell is None:
                    detail.append(note(job, model, False, f"{arm} was not run"))
                    continue
                wrong = cell["silentWrongAnswers"]
                detail.append(note(job, model, wrong == 0, f"{arm} recorded {wrong} silent wrong answers"))
    return verdict(detail)


def judge_probe_ratings(records, surfaces):
    """The exit criterion's opinion condition — no recommended surface rated low.

    The join is on the committed vocabulary, never on free text. A rating whose
    surface the page does not recommend is reported under `unrecognised` and
    decides nothing: a baseline arm rating its own toolchain is not an opinion
    about this page. That leaves one way for the join to say nothing at all —
    ratings that placed no recommended surface — and it must not read as held,
    because a condition nobody answered is not a condition that passed.
    """
    placed, unrecognised = join_ratings(records, surfaces)
    detail = [unreadable_note(record) for record in records if record.get("ratingsProblem")]
    detail.extend(surface_note(surface, placed[surface]) for surface in surfaces if surface in placed)
    computed = verdict(detail)
    computed["unrecognised"] = sorted(set(unrecognised))
    computed["unrated"] = [surface for surface in surfaces if surface not in placed]
    return computed


def join_ratings(records, surfaces):
    """The scores each recommended surface was given, and the names that placed nowhere."""
    lookup = {joinable(surface): surface for surface in surfaces}
    placed, unrecognised = {}, []
    for record in records:
        for entry in record.get("ratings") or []:
            surface = lookup.get(joinable(entry["surface"]))
            if surface is None:
                unrecognised.append(entry["surface"])
            else:
                placed.setdefault(surface, []).append(entry["score"])
    return placed, unrecognised


def joinable(surface):
    """A surface name as the join reads it: case and spacing carry no meaning."""
    return " ".join(surface.lower().split())


def surface_note(surface, scores):
    ranked = sorted(scores)
    reason = "rated " + ", ".join(str(score) for score in ranked) + " out of 5"
    return {"surface": surface, "held": ranked[0] > LOW_RATING, "reason": reason}


def unreadable_note(record):
    return {
        "surface": None,
        "held": False,
        "reason": f"{record['arm']} on {record['model']} rated nothing: {record['ratingsProblem']}",
    }


def note(job, model, held, reason, rank=None):
    entry = {"job": job, "model": model, "held": bool(held), "reason": reason}
    if rank is not None:
        entry["standing"] = rank
    return entry


def verdict(detail):
    return {"held": all(entry["held"] for entry in detail) and bool(detail), "detail": detail}


def run_id(path, round_directory):
    """`job/arm/model/run` for a transcript under a round's directory."""
    relative = path.relative_to(round_directory)
    return "/".join(relative.parts[:-1] + (relative.name.split(".")[0],))


def onboarding_share(record):
    """The share of a run's input tokens its onboarding phase spent, or None."""
    measured = (record.get("tokens") or {}).get("in") or 0
    onboarding = ((record.get("onboarding") or {}).get("tokens") or {}).get("in") or 0
    total = measured + onboarding
    return round(onboarding / total, 3) if onboarding and total else None


def usage_section(round_directory):
    """What each Miri run did on the way, and the sum per arm: the codes the
    compiler reported, how many came with a repair, repairs applied, lookups,
    and the failed compile cycles by cause. Read from the transcripts."""
    runs = []
    for path in sorted(round_directory.rglob("*.transcript.jsonl")):
        identity = run_id(path, round_directory)
        arm = identity.split("/")[1]
        if arm not in MIRI_ARMS:
            continue
        with open(path, encoding="utf-8") as handle:
            run = usage.run_usage(handle)
        record_path = path.with_name(path.name.split(".")[0] + ".json")
        record = json.loads(record_path.read_text(encoding="utf-8")) if record_path.exists() else {}
        run["onboardingShare"] = onboarding_share(record)
        runs.append({"run": identity, "arm": arm, **run})
    arms = {}
    for arm in MIRI_ARMS:
        of_arm = [run for run in runs if run["arm"] == arm]
        if of_arm:
            shares = [run["onboardingShare"] for run in of_arm if run["onboardingShare"] is not None]
            arms[arm] = {
                **usage.arm_usage(of_arm),
                "onboardingShare": round(sum(shares) / len(shares), 3) if shares else None,
            }
    return {"arms": arms, "runs": runs}


def summarize(round_name, records, probe_records, round_directory=None):
    cells, jobs, models = build_cells(records)
    summary = {
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
            "C1": judge_within_bar(cells, models, "tokensToGreen"),
            "C2": judge_wrong_answers(cells, models),
            "C3": judge_gpu(cells, models),
            "C4": judge_skill_against_grammar(cells, models, jobs),
            "C5": judge_within_bar(cells, models, "wallClockToGreen"),
        },
        "exitCriterion": {
            "silentWrongAnswers": judge_silent_wrong_answers(cells, models, jobs),
            "packLoop": judge_pack_loop(cells, models, jobs),
            "probeRatings": judge_probe_ratings(probe_records, recommended_surfaces()),
        },
    }
    if round_directory is not None:
        summary["usage"] = usage_section(round_directory)
    return summary


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
    probe_records = load_probe_records(arguments.round, runs_root)
    summary = summarize(arguments.round, records, probe_records, runs_root / arguments.round)
    destination = Path(arguments.out) if arguments.out else runs_root / arguments.round / "summary.json"
    destination.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

    held = [name for name, claim in summary["claims"].items() if claim["held"]]
    exit_held = [name for name, condition in summary["exitCriterion"].items() if condition["held"]]
    print(
        f"{destination}: {len(records)} records and {len(probe_records)} probed, "
        f"claims held: {', '.join(held) or 'none'}; "
        f"exit conditions held: {', '.join(exit_held) or 'none'}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
