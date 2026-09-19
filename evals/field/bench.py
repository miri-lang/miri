#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Run one cell of the live-model benchmark and write its record.

A cell is a job, an arm and a model. The runner prepares a scratch workspace
outside this repository, renders the brief for the arm, launches the agent
harness once with no human turn, scores what it left behind against the hidden
tests, and writes one JSON record per run.

Every number in a record is observed from outside the run: the usage the
harness reports about itself, the wall clock, and what the program the agent
wrote actually printed. Nothing is asked of the agent, because a measuring
device must not ask its subject for its own score.

    bench.py --job 02-ledger-repair --arm python --model claude-sonnet
    bench.py --job 01-word-frequency --arm miri-pack --model claude-opus --runs 3
    bench.py --job 01-word-frequency --arm rust --model claude-sonnet --dry-run
    bench.py --score-only <workspace> --job 01-word-frequency --arm rust
    bench.py --job 01-word-frequency --arm miri-pack --model claude-sonnet --probe

`--probe` runs the same cell as an unmeasured opinion probe: the subject is also
asked to rate the surfaces it used, and its records land in `runs/<round>.probe/`
beside the round, never inside it. Nothing a probe says reaches a claim; the
folder reads those records for one verdict only, on the opinion condition of the
exit criterion.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

FIELD = Path(__file__).resolve().parent
REPO = FIELD.parent.parent

# Commands that cost a toolchain invocation, counted from the transcript. They
# are reported beside the token counts and never added to them: `cargo build`
# and `miri check` are not the same unit of work.
TOOLCHAINS = ("miri", "cargo", "python3", "python", "deno", "uv", "pytest", "rustc")

MEASURED_PROMPT = (
    "Read BRIEF.md in this directory and do what it asks. "
    "You have no one to ask: finish the job on your own, and stop when it is done."
)

# What a probe asks for on top of the job, and what a measured run is never
# asked for: rating a tool buys invocations a real job would not spend.
RATINGS_FILE = "RATINGS.json"

# Where the surfaces the published page recommends are listed. The probe hands
# the subject those names so its answers can be joined against them: a free-text
# surface (`check`, `the check command`) matches nothing, and a verdict that
# joined on nothing would read as held by having asked no one.
SURFACES = REPO / "skills" / "surfaces.toml"

# The `kind` a probe record carries. `report.py` refuses a record of this kind
# rather than skipping it, so a probe copied into a round fails loudly.
PROBE_KIND = "probe"


def load_toml(path):
    with open(path, "rb") as handle:
        return tomllib.load(handle)


def arm_named(arm_id):
    for arm in load_toml(FIELD / "arms.toml")["arm"]:
        if arm["id"] == arm_id:
            return arm
    raise SystemExit(f"no arm named {arm_id} in arms.toml")


def model_named(key):
    for model in load_toml(FIELD / "models.toml")["model"]:
        if model["key"] == key:
            return model
    raise SystemExit(f"no model named {key} in models.toml")


def job_named(job_id):
    path = FIELD / "jobs" / job_id / "job.toml"
    if not path.is_file():
        raise SystemExit(f"no job named {job_id} under jobs/")
    return load_toml(path)


def render_brief(job_id, arm, job):
    """The brief the subject reads, with the three substitutions applied."""
    text = (FIELD / "jobs" / job_id / "BRIEF.md").read_text(encoding="utf-8")
    toolchain = job.get("toolchain", {}).get(arm["id"], arm["toolchain"])
    for placeholder, value in (
        ("{{LANGUAGE}}", arm["language"]),
        ("{{TOOLCHAIN}}", toolchain),
        ("{{ENTRY_POINT}}", arm["entry_point"]),
    ):
        text = text.replace(placeholder, value)
    return text


def prepare_workspace(root, job_id, arm, job):
    """Seed a scratch directory and leave the rendered brief in it.

    The workspace lives outside this repository on purpose: a subject that can
    read the compiler's own sources is not learning the language the way the
    agents this benchmark reports on will.
    """
    if root.exists():
        shutil.rmtree(root)
    seed = FIELD / "jobs" / job_id / "seeds" / seed_language(arm)
    if seed.is_dir():
        shutil.copytree(seed, root)
    else:
        root.mkdir(parents=True)
    (root / "BRIEF.md").write_text(render_brief(job_id, arm, job), encoding="utf-8")
    if arm["install_pack"]:
        install_pack(root, arm)
    return root


def seed_language(arm):
    """The seed directory an arm starts from. Both Miri arms share one."""
    return "miri" if arm["language"] == "Miri" else arm["id"]


def install_pack(root, arm):
    flavor = "claude" if arm["id"] == "miri-pack" else "agents"
    subprocess.run(
        ["miri", "skill", "install", "--agent", flavor, "--target", str(root), "--force"],
        check=True,
        capture_output=True,
    )


def recommended_surfaces():
    """The surfaces the published page recommends, commands before documents."""
    listed = load_toml(SURFACES)
    return listed["commands"] + listed["documents"]


def probe_request():
    """What a probe asks for on top of the job.

    The vocabulary is quoted into the request rather than described, so a
    subject spells a surface the way the verdict looks it up. A surface outside
    the list is still worth hearing about — a baseline arm rates its own
    toolchain — so rating one is invited rather than forbidden.
    """
    vocabulary = ", ".join(recommended_surfaces())
    return (
        f" When the job is done, write {RATINGS_FILE} in this directory. For every tool,"
        " command and document you used, rate how much it helped, from 1 (it got in the"
        " way) to 5 (the job could not have been done without it), as"
        ' {"ratings": [{"surface": "<name>", "score": <1-5>, "reason": "<one sentence>"}]}.'
        f" Where what you used is one of these, name it exactly: {vocabulary}."
        " Anything else you used, name in your own words."
    )


def prompt_for(arguments):
    return MEASURED_PROMPT + probe_request() if arguments.probe else MEASURED_PROMPT


def round_directory(arguments):
    """The directory a run's workspace and records live under.

    A probe gets a sibling of its round, never a subdirectory of it: the folder
    reads everything under `runs/<round>/`, and a probe that reused the round's
    scratch path would delete the workspace a measured record points at.
    """
    return f"{arguments.round}.{PROBE_KIND}" if arguments.probe else arguments.round


def launch_command(model, workspace, prompt):
    """The one non-interactive invocation a run is allowed."""
    if model["harness"] == "claude":
        return [
            "claude",
            "-p",
            prompt,
            "--output-format",
            "stream-json",
            "--verbose",
            "--permission-mode",
            "bypassPermissions",
            "--model",
            model["id"],
            "--add-dir",
            str(workspace),
        ]
    if model["harness"] == "gemini":
        return ["gemini", "-p", prompt, "-m", model["id"], "--yolo", "-o", "stream-json"]
    raise SystemExit(f"unknown harness {model['harness']}")


def run_agent(model, workspace, time_cap, prompt):
    """Launch the harness once, capped on wall clock, and keep its transcript."""
    started = time.monotonic()
    environment = dict(os.environ, MIRI_STDLIB_PATH=str(REPO / "src" / "stdlib"))
    try:
        finished = subprocess.run(
            launch_command(model, workspace, prompt),
            cwd=workspace,
            env=environment,
            capture_output=True,
            text=True,
            timeout=time_cap,
        )
        return finished.stdout, finished.returncode, time.monotonic() - started, False
    except subprocess.TimeoutExpired as expired:
        transcript = expired.stdout or ""
        if isinstance(transcript, bytes):
            transcript = transcript.decode("utf-8", "replace")
        return transcript, None, time.monotonic() - started, True


def parse_transcript(transcript):
    """Read tokens, turns and tool use out of a harness's own stream.

    Both harnesses stream one JSON object per line. What the two agree on is
    read here; what they do not is left absent rather than guessed, because a
    metric invented for one column would not be comparable to the other.
    """
    usage = {"in": 0, "out": 0, "observed": False}
    turns, tools, toolchain_calls = 0, 0, 0
    for line in transcript.splitlines():
        event = decode_event(line)
        if event is None:
            continue
        turns += 1 if event.get("type") == "assistant" else 0
        counted = count_usage(event)
        usage["observed"] = usage["observed"] or counted != (0, 0)
        usage["in"] += counted[0]
        usage["out"] += counted[1]
        for command in tool_commands(event):
            tools += 1
            toolchain_calls += 1 if command.split(" ")[0] in TOOLCHAINS else 0
    return usage, turns, tools, toolchain_calls


def decode_event(line):
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return None


def count_usage(event):
    """The tokens one streamed event reports.

    A harness that reports none leaves `observed` false rather than a zero. A
    cost of zero and a cost nobody measured are not the same number, and the
    folder must not treat a silent harness as a free one.
    """
    usage = event.get("message", {}).get("usage") or event.get("usage") or {}
    return int(usage.get("input_tokens", 0)), int(usage.get("output_tokens", 0))


def tool_commands(event):
    """The shell commands a streamed event carries, if any."""
    content = event.get("message", {}).get("content")
    if not isinstance(content, list):
        return []
    commands = []
    for block in content:
        if isinstance(block, dict) and block.get("type") == "tool_use":
            command = (block.get("input") or {}).get("command")
            commands.append(command if isinstance(command, str) else block.get("name", ""))
    return commands


def hidden_tests(job_id):
    directory = FIELD / "jobs" / job_id / "cases"
    return sorted(path for path in directory.iterdir() if path.suffix == ".in")


def score(workspace, job_id, arm, case_timeout=120):
    """Run every hidden test against what the agent left, after it stopped.

    The tests are run here and never before: the agent does not see them, so a
    run that passes passed on the contract rather than on the cases.
    """
    passed, failures = 0, []
    for case in hidden_tests(job_id):
        expected = case.with_suffix(".out").read_text(encoding="utf-8")
        if run_case(workspace, arm, case, case_timeout) == expected.rstrip("\n"):
            passed += 1
        else:
            failures.append(case.stem)
    return {"passed": passed, "total": len(hidden_tests(job_id)), "failures": failures}


def run_case(workspace, arm, case, case_timeout):
    try:
        finished = subprocess.run(
            [*arm["run"], str(case)],
            cwd=workspace,
            capture_output=True,
            text=True,
            timeout=case_timeout,
        )
        return finished.stdout.rstrip("\n")
    except (subprocess.TimeoutExpired, OSError):
        return None


def compiler_stamp():
    """What the subject's toolchain was, read from the repository, not the run."""
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True
    )
    version = subprocess.run(["miri", "--version"], capture_output=True, text=True)
    return commit.stdout.strip(), version.stdout.strip()


def harness_version(model):
    finished = subprocess.run([model["harness"], "--version"], capture_output=True, text=True)
    return finished.stdout.strip()


def record_for(arguments, arm, model, job, outcome):
    commit, version = compiler_stamp()
    return {
        "schemaVersion": 1,
        "round": arguments.round,
        "job": arguments.job,
        "arm": arm["id"],
        "run": outcome["run"],
        "model": model["key"],
        "modelId": model["id"],
        "harness": {"name": model["harness"], "version": harness_version(model)},
        "compilerCommit": commit,
        "compilerVersion": version,
        "packInstalled": arm["install_pack"],
        "caps": {"turns": job["turn_cap"], "timeSeconds": job["time_cap_seconds"]},
        "startedAt": outcome["startedAt"],
        "wallClockSeconds": round(outcome["wallClock"], 1),
        "tokens": outcome["tokens"],
        "turns": outcome["turns"],
        "toolInvocations": outcome["tools"],
        "toolchainInvocations": outcome["toolchainCalls"],
        # TODO: no field splits the invocations a subject spent cornering a
        # compiler defect from the ones it spent on the job. No verdict reads
        # that split — claims and the exit criterion are judged on gross cost —
        # but it tells a round what to fix, and today it is a hand count in the
        # round log. Recording it wants either crash signatures read from the
        # transcript, which miss silent wrong answers, or an attribution ledger
        # whose defects are re-run against the compiler before they count.
        "outcome": outcome["outcome"],
        "hiddenTests": outcome["hiddenTests"],
        "silentWrongAnswer": outcome["silentWrongAnswer"],
        "workspace": outcome["workspace"],
    }


def probe_record(arguments, arm, model, outcome):
    """A probe's record: who was asked, on what compiler, and what it said.

    It carries none of a measured record's cost or test fields, and it names its
    kind, so no reader can take it for a run of the round.
    """
    commit, version = compiler_stamp()
    return {
        "schemaVersion": 1,
        "kind": PROBE_KIND,
        "round": arguments.round,
        "job": arguments.job,
        "arm": arm["id"],
        "run": outcome["run"],
        "model": model["key"],
        "modelId": model["id"],
        "harness": {"name": model["harness"], "version": harness_version(model)},
        "compilerCommit": commit,
        "compilerVersion": version,
        "packInstalled": arm["install_pack"],
        "startedAt": outcome["startedAt"],
        "outcome": outcome["outcome"],
        "ratings": outcome["ratings"],
        "ratingsProblem": outcome["ratingsProblem"],
        "workspace": outcome["workspace"],
    }


def read_ratings(workspace):
    """The ratings the subject wrote, or the reason they cannot be used.

    This is the one place the benchmark reads what a subject says about itself,
    which is why a probe's records never reach a round.
    """
    path = workspace / RATINGS_FILE
    if not path.is_file():
        return None, f"{RATINGS_FILE} is missing: the subject rated nothing"
    try:
        content = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, UnicodeDecodeError) as error:
        return None, f"{RATINGS_FILE} is not JSON: {error}"
    ratings = content.get("ratings") if isinstance(content, dict) else None
    if not isinstance(ratings, list) or not ratings:
        return None, f"{RATINGS_FILE} rates no surface"
    problems = [problem for problem in map(rating_problem, ratings) if problem]
    if problems:
        return None, problems[0]
    return [normalized_rating(entry) for entry in ratings], None


def rating_problem(entry):
    surface = entry.get("surface") if isinstance(entry, dict) else None
    if not isinstance(surface, str) or not surface.strip():
        return f"{RATINGS_FILE} carries a rating that names no surface: {entry!r}"
    score = entry.get("score")
    if isinstance(score, bool) or not isinstance(score, int) or not 1 <= score <= 5:
        return f"{RATINGS_FILE} rates {surface} with score {score!r}, outside 1 to 5"
    return None


def normalized_rating(entry):
    return {"surface": entry["surface"].strip(), "score": entry["score"], "reason": str(entry.get("reason", ""))}


def outcome_of(exit_code, timed_out, turns, cap):
    if timed_out:
        return "capped"
    if turns > cap:
        return "capped"
    if exit_code == 0:
        return "finished"
    return "abandoned"


def workspace_for(arguments, arm, model, index):
    return Path(arguments.scratch) / round_directory(arguments) / arguments.job / arm["id"] / model["key"] / str(index)


def one_run(arguments, arm, model, job, index):
    workspace = workspace_for(arguments, arm, model, index)
    prepare_workspace(workspace, arguments.job, arm, job)
    started = datetime.now(timezone.utc).isoformat()
    transcript, exit_code, wall_clock, timed_out = run_agent(
        model, workspace, job["time_cap_seconds"], prompt_for(arguments)
    )
    usage, turns, tools, toolchain_calls = parse_transcript(transcript)
    tests = score(workspace, arguments.job, arm)
    outcome = outcome_of(exit_code, timed_out, turns, job["turn_cap"])
    return transcript, {
        "run": index,
        "startedAt": started,
        "wallClock": wall_clock,
        "tokens": usage,
        "turns": turns,
        "tools": tools,
        "toolchainCalls": toolchain_calls,
        "outcome": outcome,
        "hiddenTests": tests,
        "silentWrongAnswer": outcome == "finished" and tests["passed"] < tests["total"],
        "workspace": str(workspace),
    }


def one_probe(arguments, arm, model, job, index):
    """Run a cell once for its opinions. Nothing about the run is scored."""
    workspace = workspace_for(arguments, arm, model, index)
    prepare_workspace(workspace, arguments.job, arm, job)
    started = datetime.now(timezone.utc).isoformat()
    transcript, exit_code, _, timed_out = run_agent(
        model, workspace, job["time_cap_seconds"], prompt_for(arguments)
    )
    turns = parse_transcript(transcript)[1]
    ratings, problem = read_ratings(workspace)
    return transcript, {
        "run": index,
        "startedAt": started,
        "outcome": outcome_of(exit_code, timed_out, turns, job["turn_cap"]),
        "ratings": ratings,
        "ratingsProblem": problem,
        "workspace": str(workspace),
    }


def write_record(arguments, arm, model, record, transcript, index):
    directory = Path(arguments.records_root) / round_directory(arguments) / arguments.job / arm["id"] / model["key"]
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{index}.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    (directory / f"{index}.transcript.jsonl").write_text(transcript, encoding="utf-8")
    return directory / f"{index}.json"


def parse_arguments(argv):
    parser = argparse.ArgumentParser(description="run one cell of the field benchmark")
    parser.add_argument("--job", required=True)
    parser.add_argument("--arm", required=True)
    parser.add_argument("--model")
    parser.add_argument("--model-id", help="pin a model whose id models.toml leaves open")
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument("--round", default="r1")
    parser.add_argument("--scratch", default=str(Path.home() / ".cache" / "miri-field"))
    parser.add_argument(
        "--records-root",
        default=str(FIELD / "runs"),
        help="where the rounds live; this directory's runs/ by default",
    )
    parser.add_argument("--dry-run", action="store_true", help="prepare and report, launch nothing")
    parser.add_argument("--score-only", help="score an existing workspace and report")
    parser.add_argument(
        "--probe",
        action="store_true",
        help="run as an unmeasured opinion probe, into runs/<round>.probe/",
    )
    return parser.parse_args(argv)


def resolved_model(arguments):
    model = dict(model_named(arguments.model))
    if arguments.model_id:
        model["id"] = arguments.model_id
    if not model["id"]:
        raise SystemExit(
            f"model {model['key']} has no id in models.toml: pass --model-id to pin the one that ran"
        )
    return model


def dry_run(arguments, arm, job):
    workspace = Path(arguments.scratch) / round_directory(arguments) / arguments.job / arm["id"] / "dry-run"
    prepare_workspace(workspace, arguments.job, arm, job)
    print(f"workspace  {workspace}")
    print(f"cases      {len(hidden_tests(arguments.job))}")
    print(f"caps       {job['turn_cap']} turns, {job['time_cap_seconds']}s")
    if arguments.model:
        command = launch_command(resolved_model(arguments), workspace, prompt_for(arguments))
        print("launch     " + " ".join(command))


def measured_cell(arguments, arm, model, job):
    for index in range(1, arguments.runs + 1):
        transcript, outcome = one_run(arguments, arm, model, job, index)
        record = record_for(arguments, arm, model, job, outcome)
        path = write_record(arguments, arm, model, record, transcript, index)
        tests = outcome["hiddenTests"]
        print(f"{path}: {outcome['outcome']}, {tests['passed']}/{tests['total']} hidden tests")
    return 0


def probe_cell(arguments, arm, model, job):
    """Run a cell as a probe. A run whose ratings cannot be read keeps its record
    and transcript, names the problem, and fails the invocation.

    The records are what `report.py` judges the opinion condition from; nothing
    about that verdict is decided here.
    """
    unreadable = 0
    for index in range(1, arguments.runs + 1):
        transcript, outcome = one_probe(arguments, arm, model, job, index)
        record = probe_record(arguments, arm, model, outcome)
        path = write_record(arguments, arm, model, record, transcript, index)
        if outcome["ratingsProblem"]:
            unreadable += 1
            print(f"{path}: {outcome['ratingsProblem']}", file=sys.stderr)
            continue
        for rating in outcome["ratings"]:
            print(f"{path}: {rating['surface']} rated {rating['score']}/5")
    return 1 if unreadable else 0


def main(argv):
    arguments = parse_arguments(argv)
    arm = arm_named(arguments.arm)
    job = job_named(arguments.job)

    if arguments.score_only:
        if arguments.probe:
            raise SystemExit("a probe is never scored: --probe and --score-only do not combine")
        print(json.dumps(score(Path(arguments.score_only), arguments.job, arm), indent=2))
        return 0
    if arguments.dry_run:
        dry_run(arguments, arm, job)
        return 0
    if not arguments.model:
        raise SystemExit("a run needs --model; only --dry-run and --score-only may omit it")

    model = resolved_model(arguments)
    run_cell = probe_cell if arguments.probe else measured_cell
    return run_cell(arguments, arm, model, job)


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
