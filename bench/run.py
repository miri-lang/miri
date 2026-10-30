# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Run the CPU benchmark suite under the rules of `bench/CHARTER.md`.

    python3 bench/run.py --smoke                   # size S, Miri, C and Rust
    python3 bench/run.py --full --profile dev      # every workload, language and size
    python3 bench/run.py --only nbody --langs miri,c --sizes s,m --runs 5

The runner builds every port with its charter build line, proves each prints
its manifest's checksum before anything is timed, then measures fresh
processes from the outside: wall time around fork and exit, and CPU time and
peak resident memory from the child's resource usage. It never asks a program
for its score, except `hot_ns`, which only the program can know.

Every measured run becomes one JSON object in
`bench/results/<date>-<profile>/results.jsonl`, beside `env.json`, the machine
and toolchains it ran on. A results directory is never overwritten. Standard
library only, so it runs on any machine with Python 3.11 or later.
"""

import argparse
import dataclasses
import datetime
import hashlib
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import tomllib
from dataclasses import dataclass, field
from pathlib import Path

SCHEMA_VERSION = 1
BENCH = Path(__file__).resolve().parent
REPO = BENCH.parent
WORKLOADS = BENCH / "workloads"
HELLO = BENCH / "hello"
SIZES = ("s", "m", "l")
HOT_SIZES = ("s", "m")
PORT_EXTENSIONS = {
    "miri": "mi",
    "c": "c",
    "cpp": "cpp",
    "rust": "rs",
    "rust-checked": "rs",
    "go": "go",
    "python": "py",
    "typescript": "ts",
    "swift": "swift",
    "koka": "kk",
    "pypy": "py",
}
SMOKE_LANGUAGES = ("miri", "c", "rust")
# Compile time is recorded for awareness, never gated: the Miri port is
# built this many times at each of these optimisation levels, and the best
# run is reported.
COMPILE_RUNS = 3
COMPILE_OPT_LEVELS = (3, 0)
SMOKE_RUNS = 5
# The reference sets everyone else's time cap, so its own verify run is capped
# only by this, generously.
C_VERIFY_CAP_S = 3600
PROBE = """use system.io

fn classify(n int) int
    var counts = 0
    var i = 0
    while i < n
        if i % 7 < 3
            counts += 1
        else
            counts += 2
        i += 1
    counts

println(f"{classify(1000)}")
"""


# The charter and its parameters.


def charter_hash():
    """SHA-256 of CHARTER.md followed by charter.toml, as every record names it."""
    digest = hashlib.sha256()
    digest.update((BENCH / "CHARTER.md").read_bytes())
    digest.update((BENCH / "charter.toml").read_bytes())
    return digest.hexdigest()


def load_charter():
    with open(BENCH / "charter.toml", "rb") as f:
        return tomllib.load(f)


def language_order(charter):
    """Every language the charter names, subject first."""
    tiers = charter["tiers"]
    return [*tiers["subject"], *tiers["headline"], *tiers["diagnostic"], *tiers["optional"]]


# Ports and their review lines.


def review_hash(text, comment):
    """The port-checklist line's value and the SHA-256 of the file without it,
    or None when the file has no such line."""
    marker = f"{comment} port-checklist: "
    lines = text.split("\n")
    for i, line in enumerate(lines):
        if line.startswith(marker):
            without = "\n".join(lines[:i] + lines[i + 1 :])
            return line[len(marker) :], hashlib.sha256(without.encode()).hexdigest()
    return None


def review_problem(path, comment):
    """Why the port at `path` may not be timed, or None when its review holds."""
    found = review_hash(path.read_text(), comment)
    if found is None:
        return "no port-checklist line"
    review, digest = found
    if not review.endswith(f"sha256={digest}"):
        return f"the port-checklist line does not match the file (its text hashes to {digest})"
    return None


@dataclass
class Workload:
    name: str
    manifest: dict
    manifest_sha256: str

    def port(self, language):
        return WORKLOADS / self.name / f"{self.name}.{PORT_EXTENSIONS[language]}"

    def blocked(self, language):
        return self.manifest.get("blocked", {}).get(language)


def load_workloads(only):
    workloads = []
    for directory in sorted(p for p in WORKLOADS.iterdir() if p.is_dir()):
        if only and directory.name not in only:
            continue
        raw = (directory / "workload.toml").read_bytes()
        workloads.append(Workload(directory.name, tomllib.loads(raw.decode()), hashlib.sha256(raw).hexdigest()))
    return workloads


# Machine profiles: bench/machine/<profile>.toml.


def load_profile(name):
    with open(BENCH / "machine" / f"{name}.toml", "rb") as f:
        return tomllib.load(f)


def pin_words(profile):
    """The prefix that pins a process tree to the profile's core, on Linux."""
    core = profile.get("pinned_core", -1)
    if core < 0 or platform.system() != "Linux" or shutil.which("taskset") is None:
        return []
    return ["taskset", "-c", str(core)]


def machine_state(profile):
    """What this machine shows about the profile's pinned core right now."""
    core = profile.get("pinned_core", -1)
    _, turbo = frequency_state()
    return {
        "os": platform.system(),
        "can_pin": bool(pin_words(profile)),
        "governor": read_text(f"/sys/devices/system/cpu/cpu{core}/cpufreq/scaling_governor"),
        "turbo": turbo,
        "siblings": read_text(f"/sys/devices/system/cpu/cpu{core}/topology/thread_siblings_list"),
    }


def profile_problems(profile, state):
    """What keeps a machine in `state` from what a publishable profile
    declares (bench/CHARTER.md section 8). A profile is judged only on the
    state it declares: a virtual machine that exposes no governor or turbo
    control declares neither, and is never asked for them."""
    core = profile.get("pinned_core", -1)
    problems = []
    if state["os"] != "Linux":
        problems.append("a publishable run needs Linux")
    if "governor" in profile and state["governor"] != profile["governor"]:
        problems.append(f"core {core}'s governor is {state['governor']}, not {profile['governor']}")
    if "turbo" in profile and state["turbo"] is not profile["turbo"]:
        problems.append(f"turbo is {state['turbo']}, not {'on' if profile['turbo'] else 'off'}")
    if profile.get("smt_sibling_offline") and state["siblings"] != str(core):
        problems.append(f"core {core} still shares its core with {state['siblings']}")
    if not state["can_pin"]:
        problems.append("the process tree cannot be pinned (taskset is missing)")
    return problems


def publication_problems(profile):
    """What keeps this machine from the state a publishable profile declares,
    checked before every cell."""
    return profile_problems(profile, machine_state(profile))


def steal_jiffies(core, stat_text):
    """The time the hypervisor has taken from `core`, in clock ticks, from the
    text of /proc/stat, or None when the line is absent."""
    for line in stat_text.splitlines():
        fields = line.split()
        if fields and fields[0] == f"cpu{core}" and len(fields) > 8:
            return int(fields[8])
    return None


def stolen_ns(core):
    """The pinned core's steal time so far in nanoseconds, or None where the
    kernel does not report it (any machine but Linux, or no pinned core)."""
    if core < 0:
        return None
    ticks = steal_jiffies(core, read_text("/proc/stat") or "")
    return None if ticks is None else ticks * 1_000_000_000 // os.sysconf("SC_CLK_TCK")


# Commands: executed directly, never through a shell, in a scrubbed environment.


def child_environment(work_dir, extra=None):
    """Only what the charter allows a child to inherit, plus `extra`."""
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": os.environ.get("HOME", str(work_dir)),
        "PYTHONHASHSEED": "0",
        "DENO_DIR": str(work_dir / "deno"),
    }
    env.update(extra or {})
    return env


def command_words(line, src, out, programs):
    """A charter line as an argument list and the `NAME=value` assignments
    leading it; `programs` maps a program name to the path used for it."""
    words = [w.replace("{src}", str(src)).replace("{out}", str(out)) for w in line.split()]
    assignments = {}
    while words and "=" in words[0] and not words[0].startswith("-"):
        name, value = words.pop(0).split("=", 1)
        assignments[name] = value
    if words and words[0] in programs:
        words[0] = str(programs[words[0]])
    return words, assignments


def program_available(words):
    return bool(words) and (Path(words[0]).is_file() or shutil.which(words[0]) is not None)


# Measuring one process.


@dataclass
class Measurement:
    exit_code: int
    stdout: str
    stderr: str
    process_wall_ns: int
    user_ns: int
    sys_ns: int
    max_rss_bytes: int
    timed_out: bool = False
    # The time the hypervisor took from the pinned core during the run.
    steal_ns: int | None = None


def max_rss_bytes(ru_maxrss, system=None):
    """`ru_maxrss` in bytes: Linux reports kibibytes, macOS bytes."""
    system = system or platform.system()
    return ru_maxrss if system == "Darwin" else ru_maxrss * 1024


def measure(words, env, cwd, timeout_s):
    """Launch one fresh process, read its output to the end, and reap it with
    `wait4` for its resource usage. A run past `timeout_s` is killed."""
    with tempfile.TemporaryFile() as out_file, tempfile.TemporaryFile() as err_file:
        start = time.perf_counter_ns()
        child = subprocess.Popen(words, env=env, cwd=cwd, stdin=subprocess.DEVNULL, stdout=out_file, stderr=err_file)
        status, usage, timed_out = reap(child, start + int(timeout_s * 1e9))
        wall = time.perf_counter_ns() - start
        if timed_out:
            return Measurement(-9, "", "", wall, 0, 0, 0, timed_out=True)
        out_file.seek(0)
        err_file.seek(0)
        return Measurement(
            os.waitstatus_to_exitcode(status),
            out_file.read().decode(errors="replace"),
            err_file.read().decode(errors="replace"),
            wall,
            int(usage.ru_utime * 1e9),
            int(usage.ru_stime * 1e9),
            max_rss_bytes(usage.ru_maxrss),
        )


def reap(child, deadline_ns):
    """Wait for `child` with `wait4`, killing it at `deadline_ns`; returns its
    status, its resource usage and whether it was killed."""
    while True:
        pid, status, usage = os.wait4(child.pid, os.WNOHANG)
        if pid != 0:
            child.returncode = os.waitstatus_to_exitcode(status)
            return status, usage, False
        if time.perf_counter_ns() > deadline_ns:
            child.kill()
            _, status, usage = os.wait4(child.pid, 0)
            child.returncode = os.waitstatus_to_exitcode(status)
            return status, usage, True
        time.sleep(0.0005)


HOT_LINE = re.compile(r"^hot_ns=(\d+)$")
ALLOC_LINE = re.compile(r"^MIRI_ALLOC_COUNT: (\d+) allocation")
COUNTER_FIELD = re.compile(r"(\w+)=(\d+)")


def allocation_count(stderr):
    """The total the Miri runtime's allocation counter printed, or None."""
    for line in stderr.splitlines():
        if match := ALLOC_LINE.match(line.strip()):
            return int(match.group(1))
    return None


def runtime_counters(stderr):
    """The allocation breakdown and the reference-counting counts the Miri
    runtime printed at exit, as record fields."""
    fields = {}
    for line in stderr.splitlines():
        line = line.strip()
        if line.startswith("MIRI_ALLOC_COUNT:"):
            names = {"rc": "alloc_rc", "inline": "alloc_inline", "buffers": "alloc_buffer", "raw": "alloc_raw"}
        elif line.startswith("MIRI_RC_STATS:"):
            names = {"dup": "rc_dup", "drop": "rc_drop", "drop_free": "rc_drop_free", "cow_copy": "cow_copy", "realloc": "realloc"}
        else:
            continue
        for name, value in COUNTER_FIELD.findall(line):
            if name in names:
                fields[names[name]] = int(value)
    return fields


def hot_samples(stderr):
    """Every `hot_ns=<n>` line a hot run printed, in order."""
    return [int(m.group(1)) for line in stderr.splitlines() if (m := HOT_LINE.match(line.strip()))]


def coefficient_of_variation(values):
    if len(values) < 2 or statistics.mean(values) == 0:
        return 0.0
    return statistics.stdev(values) / statistics.mean(values)


def interleaved(languages, rounds):
    """The order runs are made in: every language once per round, so drift
    over the run lands on all of them alike."""
    return [(round_index, language) for round_index in range(rounds) for language in languages]


# The run's context: where things go, which compiler, which machine.


@dataclass
class Context:
    args: argparse.Namespace
    charter: dict
    charter_hash: str
    run_id: str
    work_dir: Path
    miri: Path
    records: list = field(default_factory=list)
    profile: dict = field(default_factory=dict)
    # The standard library the measured compiler reads: this tree's, unless
    # the compiler comes from another tree (`--miri-stdlib`).
    stdlib: Path = REPO / "src" / "stdlib"
    # The first diagnostic of every port that failed to build, by port path.
    build_errors: dict = field(default_factory=dict)

    def programs(self):
        return {"miri": self.miri}

    def build_env(self, language):
        extra = {"MIRI_STDLIB_PATH": str(self.stdlib)} if language == "miri" else {}
        return child_environment(self.work_dir, extra)


def git(*words):
    return subprocess.run(["git", *words], cwd=REPO, capture_output=True, text=True, check=False).stdout.strip()


def tree_is_dirty():
    """Whether anything tracked differs from the commit, ignoring results."""
    changes = git("status", "--porcelain", "--untracked-files=no").splitlines()
    return any(not line[3:].startswith("bench/results/") for line in changes)


def built_compiler(args):
    """The compiler to measure, and why it cannot be used, if it cannot.

    By default it is this tree's release build, brought up to date first with
    `make release`, which also rebuilds the release runtime libraries every
    program links: a stale binary would measure old code. An explicit
    `--miri` is used as given."""
    if args.miri:
        miri = Path(args.miri).resolve()
        return miri, None if miri.is_file() else f"{miri} does not exist"
    done = subprocess.run(["make", "release"], cwd=REPO, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        return None, "`make release` failed:\n" + done.stderr[-2000:]
    return REPO / "target" / "release" / "miri", None


def first_line(words):
    try:
        done = subprocess.run(words, capture_output=True, text=True, timeout=30, check=False)
    except (OSError, subprocess.TimeoutExpired):
        return None
    text = (done.stdout or done.stderr).strip()
    return text.splitlines()[0] if text else None


VERSION_COMMANDS = {
    "c": ["clang", "--version"],
    "cpp": ["clang++", "--version"],
    "rust": ["rustc", "--version"],
    "rust-checked": ["rustc", "--version"],
    "go": ["go", "version"],
    "python": ["python3", "--version"],
    "typescript": ["deno", "--version"],
    "swift": ["swiftc", "--version"],
    "koka": ["koka", "--version"],
    "pypy": ["pypy3", "--version"],
}

# What each language's binaries link and allocate with on its default build
# line, as the charter asks each record to say.
RUNTIME_FACTS = {
    "miri": {"stdlib": "system libc", "allocator": "libc malloc"},
    "c": {"stdlib": "system libc", "allocator": "libc malloc"},
    "cpp": {"stdlib": "libc++ (macOS) / libstdc++ (Linux)", "allocator": "libc malloc"},
    "rust": {"stdlib": "Rust std", "allocator": "system malloc"},
    "rust-checked": {"stdlib": "Rust std", "allocator": "system malloc"},
    "go": {"stdlib": "Go runtime", "allocator": "Go runtime (garbage collected)"},
    "python": {"stdlib": "CPython", "allocator": "pymalloc"},
    "typescript": {"stdlib": "Deno (V8)", "allocator": "V8 heap (garbage collected)"},
}


def toolchain_versions(ctx, languages):
    versions = {}
    for language in languages:
        command = [str(ctx.miri), "--version"] if language == "miri" else VERSION_COMMANDS.get(language)
        versions[language] = first_line(command) if command else None
    return versions


def cpu_model():
    if platform.system() == "Darwin":
        return first_line(["sysctl", "-n", "machdep.cpu.brand_string"]) or platform.processor()
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor()


def read_text(path):
    try:
        return Path(path).read_text().strip()
    except OSError:
        return None


def frequency_state():
    """The CPU frequency governor and boost state, where the OS exposes them."""
    governor = read_text("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
    boost = read_text("/sys/devices/system/cpu/cpufreq/boost")
    no_turbo = read_text("/sys/devices/system/cpu/intel_pstate/no_turbo")
    turbo = None
    if boost is not None:
        turbo = boost == "1"
    elif no_turbo is not None:
        turbo = no_turbo == "0"
    return governor, turbo


def environment(ctx, languages):
    """The machine and toolchains, with nothing that names a person or a path."""
    governor, turbo = frequency_state()
    return {
        "schema_version": SCHEMA_VERSION,
        "charter_version": ctx.charter["charter_version"],
        "charter_hash": ctx.charter_hash,
        "run_id": ctx.run_id,
        "machine_profile": ctx.args.profile,
        "publishable": bool(ctx.profile.get("publishable")),
        "pinned_core": ctx.profile.get("pinned_core", -1) if pin_words(ctx.profile) else None,
        "cpu_model": cpu_model(),
        "os": f"{platform.system()} {platform.release()}",
        "kernel": platform.version(),
        "governor": governor,
        "turbo": turbo,
        "image_digest": os.environ.get("BENCH_IMAGE_DIGEST"),
        "runner_commit": git("rev-parse", "HEAD"),
        "miri_sha": git("rev-parse", "HEAD"),
        "miri_version": first_line([str(ctx.miri), "--version"]),
        "miri_runtime_profile": runtime_profile(),
        "toolchain_versions": toolchain_versions(ctx, languages),
        "python_runner": platform.python_version(),
    }


def runtime_profile():
    """The build profile of the runtime library every Miri program links, as
    its manifest sets it: the profile's name and each setting it overrides."""
    return runtime_profile_of((REPO / "src" / "runtime" / "core" / "Cargo.toml").read_text())


def runtime_profile_of(manifest_text):
    """The release build profile a runtime manifest's text sets."""
    return {"name": "release", **tomllib.loads(manifest_text).get("profile", {}).get("release", {})}


# Building and verifying.


@dataclass
class Built:
    language: str
    run_words: list
    binary_bytes: int | None


def build(ctx, workload_name, language, src, variant):
    """Build one port, returning how to run it, or a status string."""
    spec = ctx.charter["languages"].get(language)
    if spec is None:
        return "unsupported"
    line_key = "native_build" if variant == "native" else "build"
    out = ctx.work_dir / "bin" / f"{workload_name}.{language}.{variant}"
    if "build" in spec:
        if line_key not in spec:
            return "unsupported"
        return build_with_line(ctx, language, spec[line_key], src, out)
    if variant == "native":
        return "unsupported"
    return built_port(ctx, language, src, out)


def compile_line(charter, opt_level):
    """The charter's Miri build line at `opt_level`, otherwise unchanged."""
    return charter["languages"]["miri"]["build"].replace("--opt-level 3", f"--opt-level {opt_level}")


def compile_record(ctx, env, workload, opt_level, run_index, measurement):
    """One timed build of the Miri port: the compiler's wall time and peak
    resident memory at `opt_level`."""
    record = base_record(ctx, env, workload, "miri", "default", "-", "compile")
    record.update(
        opt_level=opt_level,
        run_index=run_index,
        attempt=0,
        process_wall_ns=measurement.process_wall_ns,
        user_ns=measurement.user_ns,
        sys_ns=measurement.sys_ns,
        max_rss_bytes=measurement.max_rss_bytes,
        status="ok" if measurement.exit_code == 0 and not measurement.timed_out else "build-failed",
    )
    return record


def record_compile_times(ctx, env, workload):
    """Build the Miri port `COMPILE_RUNS` times at each level, pinned like a
    timed run, recording each build."""
    src = workload.port("miri")
    out = ctx.work_dir / "bin" / f"{workload.name}.miri.compile"
    out.parent.mkdir(parents=True, exist_ok=True)
    for opt_level in COMPILE_OPT_LEVELS:
        words, assignments = command_words(compile_line(ctx.charter, opt_level), src, out, ctx.programs())
        for run_index in range(COMPILE_RUNS):
            env_vars = ctx.build_env("miri") | assignments
            measurement = measure([*pin_words(ctx.profile), *words], env_vars, ctx.work_dir, C_VERIFY_CAP_S)
            ctx.records.append(compile_record(ctx, env, workload, opt_level, run_index, measurement))


def counted_build(ctx, workload_name, src):
    """The Miri port built for the untimed counter run: the charter's build line
    without `--release`, which leaves the runtime's counters out of the code it
    emits. The optimisation level stays the timed one, so the counts describe
    the program that was timed. Returns how to run it, or a status string."""
    words = ctx.charter["languages"]["miri"]["build"].split()
    line = " ".join(word for word in words if word != "--release")
    return build_with_line(ctx, "miri", line, src, ctx.work_dir / "bin" / f"{workload_name}.miri.counted")


def build_with_line(ctx, language, line, src, out):
    """Build one port with `line` into `out`, returning how to run it, or a
    status string."""
    out.parent.mkdir(parents=True, exist_ok=True)
    words, assignments = command_words(line, src, out, ctx.programs())
    if not program_available(words):
        return "toolchain-missing"
    done = subprocess.run(words, env=ctx.build_env(language) | assignments, cwd=ctx.work_dir, capture_output=True, check=False)
    if done.returncode != 0:
        ctx.build_errors[src] = first_diagnostic(done.stderr.decode(errors="replace") or done.stdout.decode(errors="replace"))
        return "build-failed"
    return built_port(ctx, language, src, out)


DIAGNOSTIC_CODE = re.compile(r"\b(MER_[A-Z]+_\d+|E\d{4})\b")
DIAGNOSTIC_LOCATION = re.compile(r"(?:\S*/)?([\w.-]+):(\d+)(?::\d+)?")
ABSOLUTE_PATH = re.compile(r"(?:/[^\s/:]+)+/([^\s/:]+)")
DIAGNOSTIC_DETAIL = re.compile(r"^(?:\|?\s*\^+|=)\s+(?!help:)(.+)$")


def first_diagnostic(text):
    """The first error a failed build printed, as one line that names its
    code and source line and no directory: a record never carries a path."""
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if not lines:
        return "no diagnostic was printed"
    first = next((i for i, line in enumerate(lines) if "error" in line.lower()), 0)
    message = ABSOLUTE_PATH.sub(r"\1", lines[first])
    code = DIAGNOSTIC_CODE.search(lines[first])
    location = next((m for line in lines[first:] if (m := DIAGNOSTIC_LOCATION.search(line)) and m.group(1).endswith(".mi")), None)
    detail = next((m.group(1) for line in lines[first + 1 :] if (m := DIAGNOSTIC_DETAIL.match(line))), None)
    parts = [message[:200]]
    if code and code.group(1) not in message[:200]:
        parts.append(code.group(1))
    if detail:
        parts.append(ABSOLUTE_PATH.sub(r"\1", detail)[:200])
    if location:
        parts.append(f"at {location.group(1)} line {location.group(2)}")
    return " · ".join(parts)


def built_port(ctx, language, src, out):
    """How to run the port built at `out`, or a status string."""
    run_words, _ = command_words(ctx.charter["languages"][language]["run"], src, out, ctx.programs())
    if not program_available(run_words):
        return "toolchain-missing"
    return Built(language, run_words, out.stat().st_size if out.is_file() else None)


def generator_input(workload, parameter, path):
    subprocess.run(
        [sys.executable, str(WORKLOADS / "generate.py"), workload.name, str(parameter), str(path)],
        check=True,
    )


def inputs_dir(ctx):
    """Where this run's inputs go: the profile's memory-backed directory when
    it has one, so the page cache never depends on run order."""
    root = Path(ctx.profile["input_dir"]) if ctx.profile.get("input_dir") else ctx.work_dir
    return (root if root.is_dir() else ctx.work_dir) / f"miri-bench-inputs-{ctx.run_id}"


def input_file(ctx, workload, size):
    """The input file a port reads at `size`, written once per run."""
    path = inputs_dir(ctx) / f"{workload.name}.{size}.input"
    if path.is_file():
        return path
    path.parent.mkdir(parents=True, exist_ok=True)
    entry = workload.manifest["sizes"][size]
    if workload.manifest.get("generated"):
        generator_input(workload, entry["parameter"], path)
        if hashlib.sha256(path.read_bytes()).hexdigest() != entry["input_sha256"]:
            raise SystemExit(f"the generator wrote different input for {workload.name} at size {size}")
    else:
        path.write_text(f"{entry['parameter']}\n")
    return path


# Records.


def base_record(ctx, env, workload, language, variant, size, mode):
    return {
        "schema_version": SCHEMA_VERSION,
        "charter_version": env["charter_version"],
        "charter_hash": env["charter_hash"],
        "run_id": env["run_id"],
        "machine_profile": env["machine_profile"],
        "cpu_model": env["cpu_model"],
        "os": env["os"],
        "governor": env["governor"],
        "turbo": env["turbo"],
        "image_digest": env["image_digest"],
        "runner_commit": env["runner_commit"],
        "miri_sha": env["miri_sha"],
        "miri_version": env["miri_version"],
        "toolchain_versions": env["toolchain_versions"],
        "runtime": RUNTIME_FACTS.get(language, {}),
        "workload": workload.name,
        "workload_hash": workload.manifest_sha256,
        "manifest_sha256": workload.manifest_sha256,
        "language": language,
        "variant": variant,
        "size": size,
        "mode": mode,
    }


def status_record(ctx, env, workload, language, variant, size, status, mode="cold"):
    """A cell that produced no measurements, and why: the manifest's reason
    for a blocked port, the first diagnostic for one that did not build."""
    record = base_record(ctx, env, workload, language, variant, size, mode)
    record.update(status=status, run_index=None, attempt=None)
    if status == "unsupported" and workload.blocked(language):
        record["reason"] = workload.blocked(language)
    elif status == "build-failed" and workload.port(language) in ctx.build_errors:
        record["reason"] = ctx.build_errors[workload.port(language)]
    return record


def measured_record(ctx, env, cell, run_index, attempt, measurement, floors):
    record = base_record(ctx, env, cell.workload, cell.built.language, cell.variant, cell.size, cell.mode)
    samples = hot_samples(measurement.stderr) if cell.mode == "hot" else []
    floor = floors.get(cell.built.language, {})
    record.update(
        run_index=run_index,
        attempt=attempt,
        process_wall_ns=measurement.process_wall_ns,
        user_ns=measurement.user_ns,
        sys_ns=measurement.sys_ns,
        max_rss_bytes=measurement.max_rss_bytes,
        hot_ns_samples=samples,
        hot_ns=int(statistics.median(samples)) if samples else None,
        floor_rss_bytes=floor.get("max_rss_bytes"),
        startup_ns=floor.get("process_wall_ns"),
        binary_bytes=cell.built.binary_bytes,
        exit_code=measurement.exit_code,
        steal_ns=measurement.steal_ns,
        status="timeout" if measurement.timed_out else "ok",
    )
    return record


# One cell: a workload, a language, a size, a variant and a mode.


@dataclass
class Cell:
    workload: Workload
    built: Built
    variant: str
    size: str
    mode: str
    input_path: Path

    pin: list = field(default_factory=list)

    def words(self):
        return [*self.pin, *self.built.run_words, str(self.input_path), *(["hot"] if self.mode == "hot" else [])]


def verify(ctx, cell, timeout_s):
    """Run the cell once and check its stdout is the manifest's checksum."""
    expected = cell.workload.manifest["sizes"][cell.size]["checksum"] + "\n"
    result = measure(cell.words(), child_environment(ctx.work_dir), ctx.work_dir, timeout_s)
    if result.timed_out:
        return "timeout", result
    return ("ok" if result.exit_code == 0 and result.stdout == expected else "mismatch"), result


def record_allocations(ctx, env, cell, cap_s):
    """One untimed run of the Miri port with the runtime's allocation and
    reference-counting counters on, recorded as its own `alloc-count` record:
    the counters cost time, so they never run in a measured run, and `cell`
    holds the counted build, since the timed one carries no counters."""
    env_with_counter = child_environment(ctx.work_dir, {"MIRI_ALLOC_COUNT": "1", "MIRI_RC_STATS": "1"})
    result = measure(cell.words(), env_with_counter, ctx.work_dir, cap_s)
    record = base_record(ctx, env, cell.workload, "miri", cell.variant, cell.size, "alloc-count")
    record.update(
        run_index=None,
        attempt=None,
        allocations=allocation_count(result.stderr),
        status="timeout" if result.timed_out else "ok",
        **runtime_counters(result.stderr),
    )
    ctx.records.append(record)


def time_cap(ctx, c_wall_ns):
    timeout = ctx.charter["timeout"]
    by_c = timeout["c_median_factor"] * c_wall_ns / 1e9 if c_wall_ns else 0
    return max(timeout["floor_s"], by_c)


def measured_runs(ctx, size):
    if ctx.args.runs:
        return ctx.args.runs
    return ctx.charter["runs"]["measured"][size]


def run_rounds(ctx, cells, attempt, cap_s):
    """Warm-up and measured runs of `cells`, interleaved; returns each cell's
    measurements by its index, and the indexes of cells that timed out."""
    warmup = ctx.charter["runs"]["process_warmup_runs"]
    rounds = warmup + measured_runs(ctx, cells[0].size)
    results = {i: [] for i in range(len(cells))}
    timed_out = set()
    for round_index, i in interleaved(list(range(len(cells))), rounds):
        if i in timed_out:
            continue
        measurement = measure_with_steal(ctx, cells[i], cap_s)
        if measurement.timed_out:
            measurement = measure_with_steal(ctx, cells[i], cap_s)
        if measurement.timed_out:
            timed_out.add(i)
        if round_index >= warmup or measurement.timed_out:
            results[i].append(measurement)
    return results, timed_out


def measure_with_steal(ctx, cell, cap_s):
    """One measured run, with the pinned core's steal time over it."""
    core = ctx.profile.get("pinned_core", -1) if cell.pin else -1
    before = stolen_ns(core)
    measurement = measure(cell.words(), child_environment(ctx.work_dir), ctx.work_dir, cap_s)
    after = stolen_ns(core)
    steal = after - before if before is not None and after is not None else None
    return dataclasses.replace(measurement, steal_ns=steal)


def steal_share(measurements):
    """The share of the runs' CPU time the hypervisor took, or None unmeasured."""
    stolen = [m.steal_ns for m in measurements if m.steal_ns is not None]
    cpu = sum(m.user_ns + m.sys_ns for m in measurements if m.steal_ns is not None)
    return sum(stolen) / cpu if stolen and cpu else None


def noisy(ctx, measurements, mode):
    threshold = ctx.charter["statistics"]["noisy_cv"]
    ok = [m for m in measurements if not m.timed_out]
    share = steal_share(ok)
    if share is not None and share > ctx.charter["statistics"]["steal_share_max"]:
        return True
    metrics = [[m.process_wall_ns for m in ok], [m.max_rss_bytes for m in ok]]
    if mode == "hot":
        hots = [statistics.median(s) for s in (hot_samples(m.stderr) for m in ok) if s]
        metrics.append(hots)
    return any(coefficient_of_variation(values) > threshold for values in metrics)


def measure_cells(ctx, env, cells, cap_s, floors):
    """Measure a group of cells (one workload, size and mode, every language),
    re-running noisy cells from their warm-up as the charter allows."""
    pending = list(range(len(cells)))
    for attempt in range(1 + ctx.charter["statistics"]["noisy_max_reruns"]):
        group = [cells[i] for i in pending]
        results, timed_out = run_rounds(ctx, group, attempt, cap_s)
        still_noisy = []
        for local, original in enumerate(pending):
            for run_index, m in enumerate(results[local]):
                ctx.records.append(measured_record(ctx, env, cells[original], run_index, attempt, m, floors))
            if local not in timed_out and noisy(ctx, results[local], cells[original].mode):
                still_noisy.append(original)
        pending = still_noisy
        if not pending:
            return


# Floors: what each runtime costs before the program does anything.


def measure_floors(ctx, languages, variant):
    floors = {}
    for language in languages:
        src = HELLO / f"hello.{PORT_EXTENSIONS[language]}"
        if not src.is_file():
            continue
        built = build(ctx, "hello", language, src, variant)
        if not isinstance(built, Built):
            continue
        runs = [measure(built.run_words, child_environment(ctx.work_dir), ctx.work_dir, 60) for _ in range(5)]
        floors[language] = {
            "max_rss_bytes": int(statistics.median(m.max_rss_bytes for m in runs)),
            "process_wall_ns": int(statistics.median(m.process_wall_ns for m in runs)),
        }
    return floors


# The opt-level probe: a Miri number counts only if optimization reaches code.


def opt_level_reaches_codegen(ctx):
    probe_dir = ctx.work_dir / "probe"
    probe_dir.mkdir(parents=True, exist_ok=True)
    src = probe_dir / "probe.mi"
    src.write_text(PROBE)
    objects = []
    for level in ("0", "3"):
        out = probe_dir / f"probe{level}"
        words = [str(ctx.miri), "build", "--opt-level", level, "-o", str(out), str(src)]
        subprocess.run(words, env=ctx.build_env("miri"), cwd=probe_dir, capture_output=True, check=True)
        objects.append(out.with_suffix(".o").read_bytes())
    return objects[0] != objects[1]


# The run.


def selected_languages(ctx):
    if ctx.args.langs:
        return [lang for lang in ctx.args.langs.split(",") if lang]
    if ctx.args.smoke:
        return list(SMOKE_LANGUAGES)
    return [lang for lang in language_order(ctx.charter) if lang in PORT_EXTENSIONS]


def selected_sizes(ctx):
    if ctx.args.sizes:
        return [s for s in ctx.args.sizes.split(",") if s]
    return ["s"] if ctx.args.smoke else list(SIZES)


def built_ports(ctx, env, workload, languages, variant):
    """Every port of `workload` built and reviewed, or its status recorded."""
    ready = {}
    for language in languages:
        src = workload.port(language)
        reason = None
        if workload.blocked(language):
            reason = "unsupported"
        elif not src.is_file():
            reason = "unsupported"
        elif (problem := review_problem(src, ctx.charter["languages"][language]["checklist_comment"])) is not None:
            raise SystemExit(f"{src.relative_to(REPO)}: {problem}")
        if reason is None:
            built = build(ctx, workload.name, language, src, variant)
            if isinstance(built, Built):
                ready[language] = built
                continue
            reason = built
        for size in selected_sizes(ctx):
            ctx.records.append(status_record(ctx, env, workload, language, variant, size, reason))
    return ready


def run_workload_size(ctx, env, workload, ready, size, floors):
    """Verify, then measure, every built port of `workload` at `size`."""
    input_path = input_file(ctx, workload, size)
    variant = ctx.args.variant
    modes = ["cold", *(["hot"] if size in HOT_SIZES else [])]
    c_wall = None
    verified = {}
    # C goes first: its time sets every other port's cap.
    for language in sorted(ready, key=lambda lang: lang != "c"):
        cell = Cell(workload, ready[language], variant, size, "cold", input_path, pin_words(ctx.profile))
        status, result = verify(ctx, cell, time_cap(ctx, c_wall) if language != "c" else C_VERIFY_CAP_S)
        if language == "c" and status == "ok":
            c_wall = result.process_wall_ns
        if status == "ok":
            verified[language] = ready[language]
        else:
            ctx.records.append(status_record(ctx, env, workload, language, variant, size, status))
    cap = time_cap(ctx, c_wall)
    if "miri" in verified:
        counted = counted_build(ctx, workload.name, workload.port("miri"))
        if isinstance(counted, Built):
            record_allocations(ctx, env, Cell(workload, counted, variant, size, "cold", input_path, pin_words(ctx.profile)), cap)
        else:
            ctx.records.append(status_record(ctx, env, workload, "miri", variant, size, counted, mode="alloc-count"))
    for mode in modes:
        cells = [Cell(workload, built, variant, size, mode, input_path, pin_words(ctx.profile)) for built in verified.values()]
        if cells:
            refuse_if_unfit(ctx)
            measure_cells(ctx, env, cells, cap, floors)


def refuse_if_unfit(ctx):
    """Before every cell of a publication run, the machine must still be in its
    profile's state; a run that drifts out of it stops rather than publish."""
    if ctx.profile.get("publishable") and (problems := publication_problems(ctx.profile)):
        raise SystemExit("stopping the publishable run:\n  " + "\n  ".join(problems))


def results_directory(profile, requested=None):
    """A fresh directory under bench/results, or `requested`; an existing one
    is never reused."""
    if requested:
        path = Path(requested)
        if path.exists():
            raise SystemExit(f"refusing to overwrite the results in {path}")
        path.mkdir(parents=True)
        return path
    stem = f"{datetime.date.today().isoformat()}-{profile}"
    root = BENCH / "results"
    candidate = root / stem
    n = 2
    while candidate.exists():
        candidate = root / f"{stem}-{n}"
        n += 1
    candidate.mkdir(parents=True)
    return candidate


def preflight(ctx):
    """Every reason this run may not start."""
    problems = []
    if tree_is_dirty() and not ctx.args.allow_dirty:
        problems.append("the working tree has uncommitted changes (pass --allow-dirty to run anyway)")
    if ctx.profile.get("publishable"):
        problems.extend(publication_problems(ctx.profile))
    return problems


def write_results(ctx, env):
    out = results_directory(ctx.args.profile, getattr(ctx.args, "results_dir", None))
    (out / "env.json").write_text(json.dumps(env, indent=2, sort_keys=True) + "\n")
    with open(out / "results.jsonl", "w") as f:
        for record in ctx.records:
            f.write(json.dumps(record, sort_keys=True) + "\n")
    return out


def run(args):
    charter = load_charter()
    keep_dir = REPO / "target" / "bench" if args.keep else None
    work = Path(tempfile.mkdtemp(prefix="miri-bench-")) if keep_dir is None else keep_dir
    work.mkdir(parents=True, exist_ok=True)
    run_id = datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    miri, compiler_problem = built_compiler(args)
    if compiler_problem is not None:
        raise SystemExit(f"refusing to run: {compiler_problem}")
    stdlib = Path(args.miri_stdlib).resolve() if args.miri_stdlib else REPO / "src" / "stdlib"
    ctx = Context(args, charter, charter_hash(), run_id, work, miri, profile=load_profile(args.profile), stdlib=stdlib)
    if problems := preflight(ctx):
        raise SystemExit("refusing to run:\n  " + "\n  ".join(problems))
    optimized = opt_level_reaches_codegen(ctx)
    if not optimized and not args.allow_unoptimized:
        raise SystemExit("refusing to run: --opt-level 3 emits the same machine code as --opt-level 0")
    languages = selected_languages(ctx)
    env = environment(ctx, languages) | {"opt_level_reaches_codegen": optimized}
    floors = measure_floors(ctx, languages, args.variant)
    for workload in load_workloads(args.only.split(",") if args.only else None):
        ready = built_ports(ctx, env, workload, languages, args.variant)
        if "miri" in ready:
            record_compile_times(ctx, env, workload)
        for size in selected_sizes(ctx):
            run_workload_size(ctx, env, workload, ready, size, floors)
    out = write_results(ctx, env)
    shutil.rmtree(inputs_dir(ctx), ignore_errors=True)
    if keep_dir is None:
        shutil.rmtree(work, ignore_errors=True)
    print(f"wrote {len(ctx.records)} records to {out}")
    finish(out, args, miri)
    return out


def finish(out, args, miri):
    """Summarize the run and, when asked, gather the evidence for every cell
    Miri lost. Imported here because both stages build on this module."""
    import diagnose
    import summarize

    summarize.write_summary(out)
    if args.diagnose:
        cells = diagnose.diagnose(out, miri=str(miri))
        print(f"diagnosed {len(cells)} lost cells under {(out / 'diagnosis').relative_to(REPO)}")


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--smoke", action="store_true", help="size S, Miri, C and Rust, five measured runs")
    scope.add_argument("--full", action="store_true", help="every workload, language and size (the default)")
    parser.add_argument("--only", help="comma-separated workloads")
    parser.add_argument("--langs", help="comma-separated languages")
    parser.add_argument("--sizes", help="comma-separated sizes: s, m, l")
    parser.add_argument("--runs", type=int, help="measured runs per cell, in place of the charter's")
    parser.add_argument("--profile", default="dev", choices=sorted(p.stem for p in (BENCH / "machine").glob("*.toml")))
    parser.add_argument("--variant", default="default", choices=["default", "native"])
    parser.add_argument("--keep", action="store_true", help="keep binaries and inputs under target/bench/")
    parser.add_argument("--allow-dirty", action="store_true", help="run from a tree with uncommitted changes")
    parser.add_argument("--diagnose", action="store_true", help="gather the evidence for every cell Miri loses")
    parser.add_argument("--miri", help="the compiler to measure (default: this tree's, rebuilt with `make release`)")
    parser.add_argument("--miri-stdlib", help="the standard library that compiler reads (default: this tree's src/stdlib)")
    parser.add_argument(
        "--allow-unoptimized",
        action="store_true",
        help="measure a compiler whose --opt-level does not reach its code generator, recording that in env.json",
    )
    parser.add_argument("--results-dir", help="where the records go (default: a fresh bench/results/<date>-<profile>/)")
    args = parser.parse_args(argv)
    if args.smoke and args.runs is None:
        args.runs = SMOKE_RUNS
    return args


if __name__ == "__main__":
    run(parse_args())
