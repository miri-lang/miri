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
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
from datetime import datetime, timezone
from pathlib import Path

FIELD = Path(__file__).resolve().parent
REPO = FIELD.parent.parent

# The only settings a subject's harness is launched with. The operator's own
# settings, CLAUDE.md files, hooks, plugins and MCP servers belong to no arm: a
# hook that rewrites shell output changes the very bytes being measured, and
# not equally across languages. Passed last, this file also replaces whatever
# settings a wrapper named by --harness-command passed before it.
HARNESS_SETTINGS = FIELD / "harness-settings.json"

# Environment variables that route the harness to a backend, an account or a
# configuration, or tune how the model works (a thinking budget). A subject
# inherits none of them; the account is the one the settings file's
# subscription login selects, and the model runs at the harness's defaults.
HARNESS_ROUTING_PREFIXES = ("CLAUDE", "ANTHROPIC_", "AWS_")
HARNESS_TUNING = ("MAX_THINKING_TOKENS",)

# Files the harness reads from every ancestor of its working directory.
INHERITED_MEMORY = ("CLAUDE.md", "CLAUDE.local.md")

# Commands that cost a toolchain invocation, counted from the transcript. They
# are reported beside the token counts and never added to them: `cargo build`
# and `miri check` are not the same unit of work.
TOOLCHAINS = ("miri", "cargo", "python3", "python", "uv", "pytest", "rustc")

# The workspace is named by its absolute path: an onboarded arm's measured
# session resumes the onboarding one, whose shell may have been left in a
# subdirectory (`warmup/`), and a prompt that said "this directory" then put the
# job's files where the scorer never looks.
MEASURED_PROMPT = (
    "Your workspace is {workspace}. If your shell is anywhere else, change to it "
    "first. Read BRIEF.md there and do what it asks, keeping every file it names at "
    "the path it gives, relative to {workspace}. "
    "You have no one to ask: finish the job on your own, and stop when it is done."
)

# What a probe asks for on top of the job, and what a measured run is never
# asked for: rating a tool buys invocations a real job would not spend.
RATINGS_FILE = "RATINGS.json"

# How much of a failed hidden case's stderr a record keeps.
STDERR_KEPT = 2000

# Where the surfaces the published page recommends are listed. The probe hands
# the subject those names so its answers can be joined against them: a free-text
# surface (`check`, `the check command`) matches nothing, and a verdict that
# joined on nothing would read as held by having asked no one.
SURFACES = REPO / "skills" / "surfaces.toml"

# The onboarding phase: the committed text an onboarded arm is taught from before
# its job arrives, and the materials it may be taught with. The text is the same
# for every job and names nothing job-specific; only the material sentence
# differs between arms.
ONBOARDING = FIELD / "onboarding"
ONBOARDING_TEXT = ONBOARDING / "ONBOARDING.md"
MATERIALS = ONBOARDING / "materials.toml"

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
    lines = job.get("toolchain", {})
    toolchain = lines.get(arm["id"], lines.get(arm["language"].lower(), arm["toolchain"]))
    for placeholder, value in (
        ("{{LANGUAGE}}", arm["language"]),
        ("{{TOOLCHAIN}}", toolchain),
        ("{{ENTRY_POINT}}", arm["entry_point"]),
    ):
        text = text.replace(placeholder, value)
    return text


def stage_material(root, arm):
    """Start a scratch directory holding only what the arm is taught from.

    The workspace lives outside this repository on purpose: a subject that can
    read the compiler's own sources is not learning the language the way the
    agents this benchmark reports on will. An onboarded arm's material is
    copied in by the operator's side; the subject never reads it from here.
    """
    if root.exists():
        shutil.rmtree(root)
    root.mkdir(parents=True)
    if arm["install_pack"]:
        install_pack(root, arm)
    for name in arm.get("onboarding", []):
        staged = materials()[name].get("stage")
        if staged:
            shutil.copy2(REPO / staged["from"], root / staged["to"])
    return root


def prepare_job(root, job_id, arm, job):
    """Lay the job's seed and its rendered brief into a staged workspace.

    For an onboarded arm this happens after the onboarding phase, so nothing
    job-specific is in the directory while the subject is being taught.
    """
    seed = FIELD / "jobs" / job_id / "seeds" / seed_language(arm)
    if seed.is_dir():
        shutil.copytree(seed, root, dirs_exist_ok=True)
    (root / "BRIEF.md").write_text(render_brief(job_id, arm, job), encoding="utf-8")
    return root


def prepare_workspace(root, job_id, arm, job):
    """A workspace ready for a measured prompt, with no onboarding run."""
    return prepare_job(stage_material(root, arm), job_id, arm, job)


def materials():
    return load_toml(MATERIALS)


def onboarding_prompt(arm):
    """The onboarding text with the arm's material named in it, or None for a cold arm."""
    names = arm.get("onboarding", [])
    if not names:
        return None
    described = " and ".join(materials()[name]["describes"] for name in names)
    return ONBOARDING_TEXT.read_text(encoding="utf-8").replace("{{MATERIAL}}", described)


def seed_language(arm):
    """The seed directory an arm starts from. Both Miri arms share one."""
    return "miri" if arm["language"] == "Miri" else arm["id"]


def install_pack(root, arm):
    subprocess.run(
        ["miri", "skill", "install", "--agent", "claude", "--target", str(root), "--force"],
        env=subject_environment(),
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


def prompt_for(arguments, workspace):
    measured = MEASURED_PROMPT.format(workspace=Path(workspace).resolve())
    return measured + probe_request() if arguments.probe else measured


def round_directory(arguments):
    """The directory a run's workspace and records live under.

    A probe gets a sibling of its round, never a subdirectory of it: the folder
    reads everything under `runs/<round>/`, and a probe that reused the round's
    scratch path would delete the workspace a measured record points at.
    """
    return f"{arguments.round}.{PROBE_KIND}" if arguments.probe else arguments.round


def launch_command(model, workspace, prompt, resume=None):
    """The non-interactive invocation of one phase of a run.

    A cold arm's run is this one invocation. An onboarded arm's run is two: the
    onboarding prompt, then the measured prompt resuming the same session, so
    what the subject was taught stays in its context and is paid for again,
    as cached input, on every measured turn.
    """
    if model["harness"] == "claude":
        resumed = ["--resume", resume] if resume else []
        return [
            model["command"],
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
            "--setting-sources",
            "project,local",
            "--strict-mcp-config",
            "--no-chrome",
            "--include-hook-events",
            "--settings",
            str(HARNESS_SETTINGS),
            *resumed,
        ]
    raise SystemExit(f"unknown harness {model['harness']}")


def run_agent(model, workspace, time_cap, prompt, resume=None):
    """Launch the harness once, capped on wall clock, and keep its transcript."""
    started = time.monotonic()
    try:
        finished = subprocess.run(
            launch_command(model, workspace, prompt, resume),
            cwd=workspace,
            env=subject_environment(),
            stdin=subprocess.DEVNULL,
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


# The toolchain a subject is handed, staged outside the repository by
# `stage_toolchain`. None until a run stages one.
STAGED_TOOLCHAIN = None


def subject_environment():
    """The operator's environment without what routes the harness or leads to the repository.

    Nothing in it names this repository: no variable whose value does, and no
    `PATH` entry inside it. A compiler on `PATH` inside the repository, a
    standard-library variable or even `PWD` pointing into it, is a signpost:
    a subject that looks at where its compiler lives walks into the docs, the
    examples and the hidden tests. The staged toolchain comes first on `PATH`
    instead, finds its own standard library beside the binary, and names its
    runtime libraries through `MIRI_RUNTIME_DIR`.
    """
    entries = [
        entry
        for entry in os.environ.get("PATH", "").split(os.pathsep)
        if entry and not inside_repository(entry)
    ]
    environment = {
        name: value
        for name, value in os.environ.items()
        if not name.startswith(HARNESS_ROUTING_PREFIXES)
        and name not in HARNESS_TUNING
        and not name.startswith("MIRI_")
        and not names_repository(value)
    }
    if STAGED_TOOLCHAIN is not None:
        entries.insert(0, str(STAGED_TOOLCHAIN / "bin"))
        environment["MIRI_RUNTIME_DIR"] = str(STAGED_TOOLCHAIN / "runtime")
    environment["PATH"] = os.pathsep.join(entries)
    return environment


def inside_repository(path):
    try:
        Path(path).resolve().relative_to(REPO.resolve())
        return True
    except ValueError:
        return False


def stage_toolchain(scratch):
    """Copy the operator's `miri`, its standard library and runtimes under `scratch`.

    The copy is keyed by the binary's contents, so concurrent cells share one
    staging and a rebuilt compiler gets a fresh one rather than replacing files
    a running subject is using. Returns None when there is no `miri` to stage.
    """
    binary = shutil.which("miri")
    if binary is None:
        return None
    key = hashlib.sha256(Path(binary).read_bytes()).hexdigest()[:16]
    target = Path(scratch) / "toolchain" / key
    if (target / "bin" / "miri").is_file() and staged_stdlib_is_whole(target):
        return target
    building = target.with_name(f"{key}.{os.getpid()}")
    shutil.rmtree(building, ignore_errors=True)
    (building / "bin").mkdir(parents=True)
    # `shutil.copy`, not `copy2`: a copy keeps the repository's modification
    # times otherwise, and the system's temporary-directory cleaner deletes
    # files that look days old. It emptied a staged standard library under a
    # running round once, so every later Miri run met a stdlib of four modules.
    shutil.copy(binary, building / "bin" / "miri")
    shutil.copytree(
        REPO / "src" / "stdlib", building / "lib" / "miri" / "stdlib", copy_function=shutil.copy
    )
    (building / "runtime").mkdir()
    for library in REPO.glob("src/runtime/*/target/release/libmiri_runtime_*.a"):
        shutil.copy(library, building / "runtime" / library.name)
    shutil.rmtree(target, ignore_errors=True)
    try:
        building.rename(target)
    except OSError:
        shutil.rmtree(building, ignore_errors=True)
    return target


def staged_stdlib_is_whole(target):
    """Whether a staged copy still holds every standard-library file the
    repository has; a copy something pruned is staged again."""
    source = REPO / "src" / "stdlib"
    staged = target / "lib" / "miri" / "stdlib"
    return all(
        (staged / path.relative_to(source)).is_file()
        for path in source.rglob("*.mi")
    )


def inherited_memory(scratch):
    """Every CLAUDE.md the harness would load from above a workspace under `scratch`."""
    directory = Path(scratch).resolve()
    return [
        candidate
        for folder in (directory, *directory.parents)
        for candidate in (folder / name for name in INHERITED_MEMORY)
        if candidate.is_file()
    ]


def isolation_of(transcript):
    """What the harness reported loading, and whether the run was apart from the operator.

    The harness states its MCP servers, plugins and skills in its first event
    and streams an event for every hook it runs. A run is clean when that first
    event was seen, no hook ran, no MCP server was loaded, every plugin is
    one the harness itself ships, and no tool call named this repository: a
    subject that read the compiler's docs or the hidden tests was not measured
    on the arm it was launched in.
    """
    init, hook_events, repo_references = None, 0, 0
    for line in transcript.splitlines():
        event = decode_event(line)
        if not isinstance(event, dict):
            continue
        repo_references += sum(1 for call in tool_inputs(event) if names_repository(call))
        if event.get("type") != "system":
            continue
        if event.get("subtype") == "init" and init is None:
            init = event
        elif str(event.get("subtype", "")).startswith("hook_started"):
            hook_events += 1
    init = init or {}
    servers = [server.get("name", server) if isinstance(server, dict) else server for server in init.get("mcp_servers", [])]
    plugins = [plugin for plugin in init.get("plugins", []) if isinstance(plugin, dict)]
    foreign = [plugin.get("name") for plugin in plugins if plugin.get("path") != "builtin"]
    return {
        "clean": bool(init) and hook_events == 0 and not servers and not foreign and repo_references == 0,
        "initObserved": bool(init),
        "hookEvents": hook_events,
        "repoReferences": repo_references,
        "mcpServers": servers,
        "plugins": foreign,
        "skills": init.get("skills", []),
        "apiKeySource": init.get("apiKeySource"),
    }


def parse_transcript(transcript):
    """Read tokens, turns and tool use out of a harness's own stream.

    The harness streams one JSON object per line. What it reports is read
    here; what it does not is left absent rather than guessed, because an
    invented metric would not be comparable across rounds.

    A turn is one model response. The `claude` stream splits a response into
    one event per content block, each repeating the message's id, so turns
    are counted by distinct id rather than by event.
    """
    usage = unobserved_usage()
    responses, tools, toolchain_calls = set(), 0, 0
    for line in transcript.splitlines():
        event = decode_event(line)
        if event is None:
            continue
        if event.get("type") == "assistant":
            message = event.get("message", {})
            responses.add(message.get("id") or f"event-{len(responses)}")
        if event.get("type") == "result":
            usage = settled_usage(event)
        for command in tool_commands(event):
            tools += 1
            toolchain_calls += 1 if command.split(" ")[0] in TOOLCHAINS else 0
    return usage, len(responses), tools, toolchain_calls


def decode_event(line):
    """One streamed event, or None for a line that is not a JSON object.

    The stream is mostly objects, but a harness can interleave other output;
    a bare JSON string or number on a line is not an event and is skipped.
    """
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        return None
    return event if isinstance(event, dict) else None


def unobserved_usage():
    """The cost of a run whose harness never settled its usage.

    A cost of zero and a cost nobody measured are not the same number, and the
    folder must not treat a silent harness as a free one.
    """
    return {"in": 0, "out": 0, "cacheRead": 0, "cacheCreation": 0, "observed": False}


def settled_usage(event):
    """The tokens the harness's closing `result` event reports for the run.

    Only that event is read. The usage on each streamed message is a snapshot
    taken when the message started, repeated on every content block, so
    summing it counts neither the output nor the input. `in` is every input
    token the model processed, cached or not; the cached parts are also kept
    apart, since they are billed differently.
    """
    usage = event.get("usage")
    if not isinstance(usage, dict):
        return unobserved_usage()
    cache_read = int(usage.get("cache_read_input_tokens", 0))
    cache_creation = int(usage.get("cache_creation_input_tokens", 0))
    return {
        "in": int(usage.get("input_tokens", 0)) + cache_read + cache_creation,
        "out": int(usage.get("output_tokens", 0)),
        "cacheRead": cache_read,
        "cacheCreation": cache_creation,
        "observed": True,
    }


def tool_inputs(event):
    """The serialized input of every tool call a streamed event carries."""
    content = event.get("message", {}).get("content")
    if not isinstance(content, list):
        return []
    return [
        json.dumps(block.get("input"))
        for block in content
        if isinstance(block, dict) and block.get("type") == "tool_use"
    ]


def names_repository(call):
    spellings = {str(REPO), str(REPO.resolve())}
    home = str(Path.home())
    spellings |= {"~" + spelling[len(home):] for spelling in list(spellings) if spelling.startswith(home)}
    return any(spelling in call for spelling in spellings)


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
    run that passes passed on the contract rather than on the cases. A job that
    asks for a GPU kernel is also checked for one in what the subject wrote: a
    program printing the right numbers from a loop on the processor answered a
    different job.
    """
    job = job_named(job_id)
    comparison = job.get("compare", {})
    passed, failures, detail = 0, [], {}
    for case in hidden_tests(job_id):
        expected = case.with_suffix(".out").read_text(encoding="utf-8").rstrip("\n")
        stdout, why = run_case(workspace, arm, case, case_timeout)
        difference = None if stdout is None else first_difference(expected, stdout, comparison)
        if stdout is not None and difference is None:
            passed += 1
        else:
            failures.append(case.stem)
            detail[case.stem] = why if difference is None else {**why, "firstDifference": difference}
    result = {
        "passed": passed,
        "total": len(hidden_tests(job_id)),
        "failures": failures,
        "failureDetail": detail,
    }
    if job.get("gpu", False):
        evidence = kernel_evidence(workspace, arm)
        result["kernelEvidence"] = evidence
        result["notOnGpu"] = bool(evidence["missing"])
    return result


def first_difference(expected, actual, comparison):
    """Where `actual` first departs from `expected`, or None when it does not.

    A job compares exactly unless its `[compare]` table says `kind = "numeric"`.
    Then a field that reads as a number on both sides matches within the
    tolerance — the larger of `absolute` and `relative` times the expected
    value — and every other field still matches exactly. A line whose first
    field is a word takes the tolerance `[compare.labels.<word>]` gives it, so a
    count and a mean on neighbouring lines need not share one.
    """
    if comparison.get("kind", "exact") == "exact":
        return None if actual == expected else line_difference(expected, actual)
    expected_lines, actual_lines = expected.split("\n"), actual.split("\n")
    if len(expected_lines) != len(actual_lines):
        return {"line": min(len(expected_lines), len(actual_lines)) + 1, "expected": f"{len(expected_lines)} lines", "actual": f"{len(actual_lines)} lines"}
    for number, (want, got) in enumerate(zip(expected_lines, actual_lines), start=1):
        wanted, gotten = want.split(), got.split()
        if len(wanted) != len(gotten):
            return {"line": number, "expected": want, "actual": got}
        tolerance = {**comparison, **comparison.get("labels", {}).get(wanted[0] if wanted else "", {})}
        for field, (want_field, got_field) in enumerate(zip(wanted, gotten), start=1):
            if not field_matches(want_field, got_field, tolerance):
                return {"line": number, "field": field, "expected": want_field, "actual": got_field}
    return None


def line_difference(expected, actual):
    """The first line two texts disagree on, for an exact comparison."""
    expected_lines, actual_lines = expected.split("\n"), actual.split("\n")
    for number in range(max(len(expected_lines), len(actual_lines))):
        want = expected_lines[number] if number < len(expected_lines) else None
        got = actual_lines[number] if number < len(actual_lines) else None
        if want != got:
            return {"line": number + 1, "expected": want, "actual": got}
    return None


def field_matches(expected, actual, tolerance):
    try:
        want, got = float(expected), float(actual)
    except ValueError:
        return expected == actual
    allowed = max(tolerance.get("absolute", 0.0), tolerance.get("relative", 0.0) * abs(want))
    return abs(got - want) <= allowed


# Directories a toolchain fills with code nobody in the run wrote.
VENDORED = {".venv", "target", "node_modules", "__pycache__", ".git"}


def kernel_evidence(workspace, arm):
    """Which of the arm's kernel patterns the subject's sources carry.

    An arm names, under `kernel_evidence` in `arms.toml`, the files a kernel
    would be written in and the patterns that show one is written and
    dispatched. Every pattern must match somewhere in those files; the ones
    that match nowhere are the evidence missing.
    """
    declared = arm.get("kernel_evidence", {})
    text = "\n".join(
        path.read_text(encoding="utf-8", errors="replace")
        for pattern in declared.get("files", [])
        for path in sorted(Path(workspace).glob(pattern))
        if path.is_file() and not VENDORED.intersection(path.relative_to(workspace).parts)
    )
    patterns = declared.get("patterns", [])
    missing = [pattern for pattern in patterns if not re.search(pattern, text)]
    return {"required": patterns, "missing": missing}


def run_case(workspace, arm, case, case_timeout):
    """Run one hidden case: its stdout, and how the process ended.

    The ending is kept for a failed case. Stdout alone cannot tell a wrong
    answer from a program that never compiled, trapped or ran out of time,
    and a silent-wrong-answer verdict must rest on evidence that survives.
    """
    try:
        finished = subprocess.run(
            [*arm["run"], str(case)],
            cwd=workspace,
            env=subject_environment(),
            capture_output=True,
            text=True,
            timeout=case_timeout,
        )
        ending = {"exitCode": finished.returncode, "stderr": finished.stderr[-STDERR_KEPT:]}
        return finished.stdout.rstrip("\n"), ending
    except subprocess.TimeoutExpired:
        return None, {"exitCode": None, "stderr": f"timed out after {case_timeout}s"}
    except OSError as error:
        return None, {"exitCode": None, "stderr": str(error)}


def compiler_stamp():
    """What the subject's toolchain was, read from the repository, not the run."""
    version = subprocess.run(["miri", "--version"], capture_output=True, text=True)
    return repository_commit(), version.stdout.strip()


def repository_commit():
    commit = subprocess.run(
        ["git", "rev-parse", "HEAD"], cwd=REPO, capture_output=True, text=True
    )
    return commit.stdout.strip()


def harness_version(model):
    finished = subprocess.run([model["command"], "--version"], capture_output=True, text=True)
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
        "harness": {
            "name": model["harness"],
            "command": model["command"],
            "version": harness_version(model),
        },
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
        "isolation": outcome["isolation"],
        "workspace": outcome["workspace"],
        "onboarding": outcome["onboarding"],
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
        "harness": {
            "name": model["harness"],
            "command": model["command"],
            "version": harness_version(model),
        },
        "compilerCommit": commit,
        "compilerVersion": version,
        "packInstalled": arm["install_pack"],
        "startedAt": outcome["startedAt"],
        "outcome": outcome["outcome"],
        "ratings": outcome["ratings"],
        "ratingsProblem": outcome["ratingsProblem"],
        "isolation": outcome["isolation"],
        "workspace": outcome["workspace"],
        "onboarding": outcome["onboarding"],
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


def declared_wrongly(outcome, tests):
    """A run the subject declared finished that the contract refuses: a hidden
    test fails, or the job asked for a GPU kernel and none was written."""
    return outcome == "finished" and (tests["passed"] < tests["total"] or tests.get("notOnGpu", False))


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


def session_of(transcript):
    """The session id the harness reported, which a later phase resumes."""
    for line in transcript.splitlines():
        event = decode_event(line)
        if isinstance(event, dict) and isinstance(event.get("session_id"), str):
            return event["session_id"]
    return None


def onboard(arm, model, workspace):
    """Run the onboarding phase, unmeasured, and return what it cost apart.

    Its metrics never enter the measured ones: they are kept beside them under
    `onboarding`, so a reader can add them back, and the measured phase still
    pays for the onboarding context as cached input on every turn.
    """
    prompt = onboarding_prompt(arm)
    settings = load_toml(MATERIALS).get("phase", {})
    transcript, exit_code, wall_clock, timed_out = run_agent(
        model, workspace, settings.get("time_cap_seconds", 900), prompt
    )
    session = session_of(transcript)
    if session is None:
        raise SystemExit(f"the onboarding phase in {workspace} reported no session to resume")
    usage, turns, tools, toolchain_calls = parse_transcript(transcript)
    return transcript, session, {
        "material": arm["onboarding"],
        "tokens": usage,
        "turns": turns,
        "toolInvocations": tools,
        "toolchainInvocations": toolchain_calls,
        "wallClockSeconds": round(wall_clock, 1),
        "outcome": outcome_of(exit_code, timed_out, turns, settings.get("turn_cap", 40)),
        "isolation": isolation_of(transcript),
        "sessionId": session,
    }


def launch(arguments, arm, model, job, index):
    """Stage a run's workspace, onboard it when the arm is taught, and run the job.

    Returns the measured phase's transcript and ending, and the onboarding
    phase's transcript and summary (None and None for a cold arm).
    """
    workspace = stage_material(workspace_for(arguments, arm, model, index), arm)
    onboarding_transcript, session, onboarding = None, None, None
    if onboarding_prompt(arm):
        onboarding_transcript, session, onboarding = onboard(arm, model, workspace)
    prepare_job(workspace, arguments.job, arm, job)
    started = datetime.now(timezone.utc).isoformat()
    transcript, exit_code, wall_clock, timed_out = run_agent(
        model, workspace, job["time_cap_seconds"], prompt_for(arguments, workspace), session
    )
    return workspace, started, transcript, exit_code, wall_clock, timed_out, onboarding_transcript, onboarding


def isolation_of_run(transcript, onboarding):
    """The measured phase's isolation; a run is clean only if both phases were."""
    isolation = isolation_of(transcript)
    if onboarding is not None:
        isolation["clean"] = isolation["clean"] and onboarding["isolation"]["clean"]
    return isolation


def one_run(arguments, arm, model, job, index):
    workspace, started, transcript, exit_code, wall_clock, timed_out, onboarding_transcript, onboarding = launch(
        arguments, arm, model, job, index
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
        "silentWrongAnswer": declared_wrongly(outcome, tests),
        "isolation": isolation_of_run(transcript, onboarding),
        "workspace": str(workspace),
        "onboarding": onboarding,
        "onboardingTranscript": onboarding_transcript,
    }


def one_probe(arguments, arm, model, job, index):
    """Run a cell once for its opinions. Nothing about the run is scored."""
    workspace, started, transcript, exit_code, _, timed_out, onboarding_transcript, onboarding = launch(
        arguments, arm, model, job, index
    )
    turns = parse_transcript(transcript)[1]
    ratings, problem = read_ratings(workspace)
    return transcript, {
        "run": index,
        "startedAt": started,
        "outcome": outcome_of(exit_code, timed_out, turns, job["turn_cap"]),
        "ratings": ratings,
        "ratingsProblem": problem,
        "isolation": isolation_of_run(transcript, onboarding),
        "workspace": str(workspace),
        "onboarding": onboarding,
        "onboardingTranscript": onboarding_transcript,
    }


def write_record(arguments, arm, model, record, transcript, index, onboarding_transcript=None):
    directory = Path(arguments.records_root) / round_directory(arguments) / arguments.job / arm["id"] / model["key"]
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{index}.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    (directory / f"{index}.transcript.jsonl").write_text(transcript, encoding="utf-8")
    if onboarding_transcript is not None:
        (directory / f"{index}.onboarding.jsonl").write_text(onboarding_transcript, encoding="utf-8")
    return directory / f"{index}.json"


def parse_arguments(argv):
    parser = argparse.ArgumentParser(description="run one cell of the field benchmark")
    parser.add_argument("--job", required=True)
    parser.add_argument("--arm", required=True)
    parser.add_argument("--model")
    parser.add_argument("--model-id", help="pin a model whose id models.toml leaves open")
    parser.add_argument(
        "--harness-command",
        help="the executable that runs the harness, when it is not the harness's own name "
        "(a wrapper that selects an account, say); every record names the one that ran",
    )
    parser.add_argument("--runs", type=int, default=1)
    parser.add_argument(
        "--from-run",
        type=int,
        default=1,
        help="the first run number; a cell an interrupted round left short resumes here",
    )
    parser.add_argument("--round", default="r1")
    parser.add_argument(
        "--scratch",
        default=str(Path(tempfile.gettempdir()) / "miri-field"),
        help="where workspaces are made; no ancestor of it may hold a CLAUDE.md",
    )
    parser.add_argument(
        "--records-root",
        default=str(FIELD / "runs"),
        help="where the rounds live; this directory's runs/ by default",
    )
    parser.add_argument("--dry-run", action="store_true", help="prepare and report, launch nothing")
    parser.add_argument("--score-only", help="score an existing workspace and report")
    parser.add_argument(
        "--rescore",
        help="score a recorded run's workspace again and rewrite its record, keeping the old score",
    )
    parser.add_argument(
        "--probe",
        action="store_true",
        help="run as an unmeasured opinion probe, into runs/<round>.probe/",
    )
    return parser.parse_args(argv)


def resolved_model(arguments):
    model = dict(model_named(arguments.model))
    model["command"] = arguments.harness_command or model["harness"]
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
    taught = onboarding_prompt(arm)
    if taught:
        print("onboarding " + taught.replace("\n", "\n           "))
    if arguments.model:
        model = resolved_model(arguments)
        if taught:
            print("onboard    " + " ".join(launch_command(model, workspace, taught)))
        resume = "<onboarding session>" if taught else None
        print("launch     " + " ".join(launch_command(model, workspace, prompt_for(arguments, workspace), resume)))


def measured_cell(arguments, arm, model, job):
    for index in range(arguments.from_run, arguments.from_run + arguments.runs):
        transcript, outcome = one_run(arguments, arm, model, job, index)
        record = record_for(arguments, arm, model, job, outcome)
        path = write_record(arguments, arm, model, record, transcript, index, outcome["onboardingTranscript"])
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
    for index in range(arguments.from_run, arguments.from_run + arguments.runs):
        transcript, outcome = one_probe(arguments, arm, model, job, index)
        record = probe_record(arguments, arm, model, outcome)
        path = write_record(arguments, arm, model, record, transcript, index, outcome["onboardingTranscript"])
        if outcome["ratingsProblem"]:
            unreadable += 1
            print(f"{path}: {outcome['ratingsProblem']}", file=sys.stderr)
            continue
        for rating in outcome["ratings"]:
            print(f"{path}: {rating['surface']} rated {rating['score']}/5")
    return 1 if unreadable else 0


def rescore(record_path, arguments, arm):
    """Score a recorded run again, after a defect in the scorer itself.

    Only the hidden tests are run again, against the workspace the subject
    left; nothing the agent did is re-measured. The score it replaces stays in
    the record under `rescored.previous`, with the commit that rescored it, so
    the correction is visible to anyone reading the round.
    """
    record = json.loads(record_path.read_text(encoding="utf-8"))
    if record.get("job") != arguments.job or record.get("arm") != arguments.arm:
        raise SystemExit(f"{record_path} is not a {arguments.job} / {arguments.arm} record")
    tests = score(Path(record["workspace"]), arguments.job, arm)
    record["rescored"] = {
        "at": datetime.now(timezone.utc).isoformat(),
        "commit": repository_commit(),
        "run": arm["run"],
        "previous": {
            "hiddenTests": record["hiddenTests"],
            "silentWrongAnswer": record["silentWrongAnswer"],
        },
    }
    record["hiddenTests"] = tests
    record["silentWrongAnswer"] = declared_wrongly(record["outcome"], tests)
    record_path.write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    print(f"{record_path}: {record['outcome']}, {tests['passed']}/{tests['total']} hidden tests (rescored)")
    return 0


def main(argv):
    arguments = parse_arguments(argv)
    arm = arm_named(arguments.arm)
    job = job_named(arguments.job)
    global STAGED_TOOLCHAIN
    STAGED_TOOLCHAIN = stage_toolchain(arguments.scratch)

    if arguments.score_only:
        if arguments.probe:
            raise SystemExit("a probe is never scored: --probe and --score-only do not combine")
        print(json.dumps(score(Path(arguments.score_only), arguments.job, arm), indent=2))
        return 0
    if arguments.rescore:
        return rescore(Path(arguments.rescore), arguments, arm)
    found = inherited_memory(arguments.scratch)
    if found:
        raise SystemExit(
            "a subject in this scratch directory would load the operator's instructions from "
            + ", ".join(str(path) for path in found)
            + "; pass a --scratch outside it"
        )
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
