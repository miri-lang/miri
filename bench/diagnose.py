# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Gather the evidence for every cell Miri lost, so no loss is explained by
prose alone.

    python3 bench/diagnose.py bench/results/<run>/                  # every cell summary.json marks
    python3 bench/diagnose.py bench/results/<run>/ --cell nbody/s   # one workload and size

For each lost cell it writes `diagnosis/<workload>-<size>/` beside the run:

- `counters.json`: hardware counters for Miri, C and Rust (`perf stat` on
  Linux; elsewhere the reason they are unavailable);
- `profile.<language>.txt`, `folded.<language>.txt`, `flamegraph.<language>.svg`
  and `top.<language>.json`: a sampled profile of a hot run, its stacks folded,
  drawn, and its top self-time symbols. The profiler is `sample` on macOS and
  `perf record` on Linux; both fold into the same stacks. Miri is sampled from
  its `--profile` build, which keeps a frame pointer in every function so the
  profiler sees every caller; the counters stay on the release build that was
  timed;
- `miri.mir` and `miri.clif`: what the compiler made of the Miri port, and
  `hot.clif` and `hot.asm`: the CLIF and machine code of its hottest functions;
- `runtime.json`: the Miri runtime's allocation counts;
- `DIAGNOSIS.md`: the evidence side by side, with an empty hypothesis and an
  empty task for the engineer to fill, and the build each artifact came
  from. The tool gathers evidence; a person
  names the mechanism, files the work, and adds it to `diagnosis/CATALOG.md`.

Standard library only; the profiler is the operating system's own.
"""

import argparse
import html
import json
import os
import platform
import re
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

import run

LANGUAGES = ("miri", "c", "rust")
PERF_EVENTS = "cycles,instructions,branches,branch-misses,cache-misses,L1-dcache-load-misses"
SAMPLE_SECONDS = 3
SAMPLE_HZ = 999
TOP_SYMBOLS = 15
HOT_FUNCTIONS = 3


# Choosing cells.


def lost_cells(summary, only):
    """(workload, size) of every Miri cell the summary marks for diagnosis,
    or the one `only` names as `workload/size`."""
    if only:
        workload, size = only.split("/")
        return [(workload, size)]
    cells = {(c["workload"], c["size"]) for c in summary["cells"] if c["language"] == "miri" and c.get("diagnose")}
    return sorted(cells)


# Building the ports, with the compiler's listings for Miri.


@dataclass
class Ports:
    """The builds of one workload's ports. `timed` is each language's release
    build, the one the counters run; `sampled` is the build the profiler
    samples, the same build except Miri's, or a status string when that build
    failed. `listing` is where Miri's MIR and CLIF listings were written."""

    timed: dict
    sampled: dict
    listing: Path


def build_ports(ctx, workload, work_dir):
    """Each language's built port, timed and sampled; Miri's listing build
    also writes its MIR and CLIF listings beside the executable."""
    timed = {}
    for language in LANGUAGES:
        src = workload.port(language)
        if not src.is_file() or workload.blocked(language):
            continue
        result = run.build(ctx, workload.name, language, src, "default")
        if isinstance(result, run.Built):
            timed[language] = result
    sampled = dict(timed)
    if "miri" in timed:
        sampled["miri"] = profile_build(ctx, workload.name, workload.port("miri"))
    miri_out = ctx.work_dir / "bin" / f"{workload.name}.listing"
    words = [str(ctx.miri), "build", "--release", "--opt-level", "3", "--emit", "mir", "--emit", "clif", "-o", str(miri_out), str(workload.port("miri"))]
    subprocess.run(words, env=ctx.build_env("miri"), cwd=work_dir, capture_output=True, check=False)
    return Ports(timed, sampled, miri_out)


def profile_build(ctx, workload_name, src):
    """The Miri port built for the profiler: the charter's build line with
    `--profile` in place of `--release`, which is the same release code with a
    frame pointer kept in every function, the runtime's included. Returns how
    to run it, or a status string."""
    words = ctx.charter["languages"]["miri"]["build"].split()
    line = " ".join("--profile" if word == "--release" else word for word in words)
    return run.build_with_line(ctx, "miri", line, src, ctx.work_dir / "bin" / f"{workload_name}.miri.profile")


# Hardware counters.


def counters(built, input_path, env):
    """`perf stat` per language, or why there are none."""
    if platform.system() != "Linux" or shutil.which("perf") is None:
        reason = "perf is Linux-only" if platform.system() != "Linux" else "perf is not installed"
        return {"available": False, "reason": f"{reason}; the sampled profile stands in"}
    out = {"available": True, "events": PERF_EVENTS, "languages": {}}
    for language, port in built.items():
        words = ["perf", "stat", "-x", ",", "-e", PERF_EVENTS, *port.run_words, str(input_path)]
        done = subprocess.run(words, env=env, capture_output=True, text=True, check=False)
        out["languages"][language] = perf_values(done.stderr)
    return out


def perf_values(stderr):
    values = {}
    for line in stderr.splitlines():
        fields = line.split(",")
        if len(fields) > 2 and fields[0].replace(".", "").isdigit():
            values[fields[2]] = float(fields[0])
    if "instructions" in values and values.get("cycles"):
        values["ipc"] = values["instructions"] / values["cycles"]
    return values


# Sampling a hot run.


@dataclass
class Sampled:
    """A sampled profile of a hot run: the profiler's own report, and its
    stacks folded."""

    report: str
    folded: dict


def sample_profile(port, input_path, env, work_dir):
    """A sampled call graph of the port's hot run, or why there is none."""
    system = platform.system()
    if system == "Darwin":
        return sample_on_macos(port, input_path, env, work_dir)
    if system == "Linux":
        return sample_on_linux(port, input_path, env, work_dir)
    return f"no sampling profiler is supported on {system}"


def start_hot_run(port, input_path, env, work_dir):
    """The port's hot run, started and given a moment to get past start-up."""
    child = subprocess.Popen([*port.run_words, str(input_path), "hot"], env=env, cwd=work_dir, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.2)
    return child


def stop_hot_run(child):
    child.kill()
    child.wait()


def sample_on_macos(port, input_path, env, work_dir):
    sampler = shutil.which("sample")
    if sampler is None:
        return "`sample` is not installed"
    child = start_hot_run(port, input_path, env, work_dir)
    out = work_dir / f"sample.{child.pid}.txt"
    subprocess.run([sampler, str(child.pid), str(SAMPLE_SECONDS), "-mayDie", "-file", str(out)], capture_output=True, check=False)
    stop_hot_run(child)
    if not out.is_file():
        return "`sample` wrote no profile"
    report = out.read_text()
    return sampled_or_reason(scrubbed(report, work_dir), folded_stacks(call_tree(report)))


def sample_on_linux(port, input_path, env, work_dir):
    """`perf record` of the hot run's user-space stacks, walked by frame
    pointer; `perf script` folds it and `perf report` is its readable form."""
    problem = perf_problem()
    if problem is not None:
        return problem
    child = start_hot_run(port, input_path, env, work_dir)
    data = work_dir / f"perf.{child.pid}.data"
    record = ["perf", "record", "--all-user", "-F", str(SAMPLE_HZ), "--call-graph", "fp", "-o", str(data), "-p", str(child.pid), "--", "sleep", str(SAMPLE_SECONDS)]
    recorded = subprocess.run(record, capture_output=True, text=True, check=False)
    stop_hot_run(child)
    # perf also exits non-zero when the process it is attached to ends inside
    # the window, so its samples, not its exit status, decide.
    script = subprocess.run(["perf", "script", "-i", str(data), "-F", "comm,tid,ip,sym,dso"], capture_output=True, text=True, check=False) if data.is_file() else None
    stacks, note = demangled(script.stdout) if script else ("", "")
    folded = folded_perf_stacks(stacks)
    if not folded and (recorded.returncode != 0 or script is None):
        return f"perf record failed{paranoid_note()}: {first_lines(scrubbed(recorded.stderr, work_dir))}"
    if not folded:
        return "perf recorded no samples of the hot run"
    report_words = ["perf", "report", "-i", str(data), "--stdio", "--no-children", "--sort", "dso,symbol", "-g", "none", "--percent-limit", "0.5"]
    report, _ = demangled(subprocess.run(report_words, capture_output=True, text=True, check=False).stdout)
    return Sampled(scrubbed(f"# {note}\n{report}" if note else report, work_dir), folded)


def demangled(text):
    """`text` with its Rust (legacy and v0) and C++ symbol names demangled by
    `c++filt`, and a note when they could not be, in which case the text is
    returned as it was. perf demangles C++ itself but not Rust's v0 scheme."""
    filt = shutil.which("c++filt")
    if filt is None:
        return text, "symbols are left mangled: c++filt is not installed"
    done = subprocess.run([filt], input=text, capture_output=True, text=True, check=False)
    if done.returncode != 0:
        return text, "symbols are left mangled: c++filt failed"
    return done.stdout, ""


def sampled_or_reason(report, folded):
    return Sampled(report, folded) if folded else "the profiler recorded no samples of the hot run"


def scrubbed(text, work_dir):
    """`text` without this machine's scratch, checkout and home directories,
    which profilers, compilers and disassemblers print in the paths they
    name. The longest is replaced first, since one may hold another."""
    names = {str(work_dir): "<work>", str(run.REPO): "<repo>", str(Path.home()): "~"}
    for path in sorted((path for path in names if path not in ("", "/")), key=len, reverse=True):
        text = text.replace(path, names[path])
    return text


def scrub_bundle(out_dir, work_dir):
    """Every text artifact of the bundle, scrubbed of this machine's paths."""
    for path in out_dir.iterdir():
        try:
            text = path.read_text()
        except (OSError, UnicodeDecodeError):
            continue
        if (clean := scrubbed(text, work_dir)) != text:
            path.write_text(clean)


def first_lines(text, count=3):
    return " ".join([line.strip() for line in text.splitlines() if line.strip()][:count]) or "no message"


# Whether `perf` may sample here. Sampling another process's user-space
# stacks needs `kernel.perf_event_paranoid` at 2 or less; Debian and Ubuntu
# kernels add levels above 2 that refuse unprivileged users altogether.
PERF_PARANOID = Path("/proc/sys/kernel/perf_event_paranoid")
PERF_USER_SAMPLING_LEVEL = 2


def perf_problem(paranoid_file=PERF_PARANOID):
    """Why `perf` cannot sample a process's user stacks here, or None when
    nothing stands in its way that can be seen before trying."""
    if shutil.which("perf") is None:
        return "perf is not installed"
    level = paranoid_level(paranoid_file)
    if level is not None and level > PERF_USER_SAMPLING_LEVEL:
        return f"kernel.perf_event_paranoid is {level}; sampling a process's user stacks needs {PERF_USER_SAMPLING_LEVEL} or less (as root: sysctl kernel.perf_event_paranoid={PERF_USER_SAMPLING_LEVEL})"
    return None


def paranoid_level(paranoid_file=PERF_PARANOID):
    try:
        return int(paranoid_file.read_text().strip())
    except (OSError, ValueError):
        return None


def paranoid_note():
    level = paranoid_level()
    return f" (kernel.perf_event_paranoid is {level})" if level is not None else ""


# `perf script -F comm,tid,ip,sym,dso` prints each sample as a `comm tid`
# line followed by its frames, innermost first, one `address symbol (dso)`
# line each. An address in the upper half belongs to the kernel, which
# user-space sampling reaches only through the entry into it.
PERF_FRAME = re.compile(r"^\s+(?P<address>[0-9a-f]+) (?P<symbol>.*) \((?P<dso>[^)]*)\)$")
KERNEL_FRAME = "[kernel]"
KERNEL_ADDRESSES = 1 << 63
UNKNOWN_SYMBOL = "[unknown]"


def folded_perf_stacks(script):
    """`perf script` output in the folded format `folded_stacks` writes: the
    thread's name at the root, then every frame outermost first."""
    folded = {}
    for thread, frames in perf_samples(script):
        if frames:
            key = ";".join([thread, *reversed(frames)])
            folded[key] = folded.get(key, 0) + 1
    return folded


def perf_samples(script):
    """(thread name, frames innermost first) of every sample."""
    samples = []
    for line in script.splitlines():
        if not line.strip():
            continue
        if not line[0].isspace():
            samples.append((line.rsplit(maxsplit=1)[0].strip(), []))
        elif samples and (match := PERF_FRAME.match(line)):
            frame = perf_frame_name(match.group("address"), match.group("symbol"), match.group("dso"))
            frames = samples[-1][1]
            if not (frame == KERNEL_FRAME and frames and frames[-1] == KERNEL_FRAME):
                frames.append(frame)
    return samples


def perf_frame_name(address, symbol, dso):
    """A frame's name in a folded stack: its symbol, or the library it is in
    when perf could not name it, never a path."""
    if int(address, 16) >= KERNEL_ADDRESSES:
        return KERNEL_FRAME
    if symbol != UNKNOWN_SYMBOL:
        return symbol.replace(";", ":")
    return f"[{Path(dso).name}]" if dso != UNKNOWN_SYMBOL else UNKNOWN_SYMBOL


# A sample whose stack the profiler could not walk past the loader's entry
# point: code compiled without frame pointers looks like this. It is reported
# under its own name rather than as time spent in the loader.
UNWALKED_ROOT = "_dyld_start"
UNWALKABLE = "[stack not walkable]"

GRAPH_LINE = re.compile(r"^(?P<lead>[ +!:|]*?)(?P<count>\d+) (?P<name>.+?)(?:  \(in [^)]*\).*)?$")


def call_tree(profile):
    """The profile's call graph as (depth, count, name) rows, in order."""
    rows = []
    in_graph = False
    for line in profile.splitlines():
        if line.startswith("Call graph:"):
            in_graph = True
            continue
        if in_graph and not line.strip():
            break
        if in_graph and (match := GRAPH_LINE.match(line)):
            rows.append((len(match.group("lead")), int(match.group("count")), match.group("name").strip()))
    return rows


def folded_stacks(rows):
    """Brendan Gregg's folded format: one `a;b;c count` line per stack, the
    count being the samples whose innermost frame is `c`."""
    stack = []
    folded = {}
    for i, (depth, count, name) in enumerate(rows):
        while stack and stack[-1][0] >= depth:
            stack.pop()
        stack.append((depth, name))
        own = count - direct_children_samples(rows, i)
        if own > 0:
            key = ";".join(n for _, n in stack)
            if name == UNWALKED_ROOT and len(stack) <= 2:
                key = f"{stack[0][1]};{UNWALKABLE}"
            folded[key] = folded.get(key, 0) + own
    return folded


def direct_children_samples(rows, index):
    """The samples of the row at `index` that its direct callees account for."""
    depth = rows[index][0]
    child_depth = None
    total = 0
    for later_depth, later_count, _ in rows[index + 1 :]:
        if later_depth <= depth:
            break
        if child_depth is None:
            child_depth = later_depth
        if later_depth == child_depth:
            total += later_count
    return total


def top_symbols(folded):
    """The symbols with the most samples at the top of the stack."""
    totals = {}
    for stack, count in folded.items():
        leaf = stack.rsplit(";", 1)[-1]
        totals[leaf] = totals.get(leaf, 0) + count
    whole = sum(totals.values()) or 1
    ranked = sorted(totals.items(), key=lambda item: -item[1])[:TOP_SYMBOLS]
    return [{"symbol": name, "samples": samples, "share": samples / whole} for name, samples in ranked]


# Drawing the flamegraph.


def flamegraph_svg(folded, title, width=1200, row=16):
    """A flamegraph of folded stacks: the root at the bottom, each frame as wide
    as its samples."""
    tree = {}
    for stack, count in folded.items():
        node = tree
        for frame in stack.split(";"):
            entry = node.setdefault(frame, [0, {}])
            entry[0] += count
            node = entry[1]
    total = sum(entry[0] for entry in tree.values()) or 1
    depth = max((stack.count(";") + 1 for stack in folded), default=1)
    height = (depth + 2) * row
    rects = []

    def draw(node, x, level):
        for name, (count, children) in sorted(node.items()):
            w = width * count / total
            if w >= 0.5:
                y = height - (level + 1) * row
                label = html.escape(name)
                hue = 20 + (hash(name) % 40)
                rects.append(
                    f'<g><title>{label} ({count} samples)</title><rect x="{x:.1f}" y="{y}" width="{w:.1f}" height="{row - 1}" fill="hsl({hue},80%,60%)"/>'
                    + (f'<text x="{x + 3:.1f}" y="{y + row - 4}" font-size="11">{label[: int(w / 7)]}</text>' if w > 30 else "")
                    + "</g>"
                )
                draw(children, x, level + 1)
            x += w

    draw(tree, 0.0, 0)
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" font-family="monospace">'
        f'<text x="4" y="14" font-size="13">{html.escape(title)}</text>' + "".join(rects) + "</svg>\n"
    )


# The compiler's listings of the hottest functions.


def listing_sections(text, marker):
    """A listing split into its functions: name to text. `marker` starts each
    function's section."""
    sections = {}
    current = None
    for line in text.splitlines(keepends=True):
        if line.startswith(marker):
            current = line[len(marker) :].strip()
            sections[current] = ""
        if current is not None:
            sections[current] += line
    return sections


# A Miri function's link name: `miri.` before a function of the program's own
# module, `miri$` before the path of any other module's, the standard library's
# included. The runtime's `miri_rt_` symbols are neither.
MIRI_FUNCTION_PREFIXES = ("miri.", "miri$")


def hot_miri_functions(top):
    return [entry["symbol"] for entry in top if entry["symbol"].startswith(MIRI_FUNCTION_PREFIXES)][:HOT_FUNCTIONS]


def disassembly(binary, symbols):
    """Machine code of `symbols` in `binary`, through objdump."""
    tool = disassembler()
    if tool is None:
        return "unavailable: objdump is not installed\n"
    prefix = "_" if platform.system() == "Darwin" else ""
    listings = []
    for words in disassembly_commands(*tool, binary, [prefix + s for s in symbols]):
        done = subprocess.run(words, capture_output=True, text=True, check=False)
        listings.append(done.stdout or f"unavailable: {done.stderr.strip()}\n")
    # objdump names the binary by its path, which is a scratch directory of
    # this machine's; the bundle names it by the workload instead.
    return "".join(listings).replace(str(binary), binary.name)


def disassembler():
    """(objdump, flavour): llvm-objdump when installed, under its own name or
    a versioned one, else the system objdump, `gnu` when it is GNU's. None
    when there is neither."""
    llvm = shutil.which("llvm-objdump") or newest_versioned("llvm-objdump")
    if llvm is not None:
        return llvm, "llvm"
    objdump = shutil.which("objdump")
    if objdump is None:
        return None
    version = subprocess.run([objdump, "--version"], capture_output=True, text=True, check=False).stdout
    return objdump, "gnu" if "GNU" in version else "llvm"


def newest_versioned(name):
    """The `name-<major>` executable on PATH with the highest major version."""
    found = []
    for directory in filter(None, os.environ.get("PATH", "").split(os.pathsep)):
        for path in Path(directory).glob(f"{name}-[0-9]*"):
            suffix = path.name[len(name) + 1 :]
            if suffix.isdigit() and os.access(path, os.X_OK):
                found.append((int(suffix), str(path)))
    return max(found)[1] if found else None


def disassembly_commands(objdump, flavour, binary, symbols):
    """The objdump runs that print `symbols`: llvm-objdump takes them in one
    list, GNU objdump only one `--disassemble=` at a time (a second replaces
    the first)."""
    if flavour == "gnu":
        return [[objdump, "-d", f"--disassemble={symbol}", str(binary)] for symbol in symbols]
    return [[objdump, "-d", f"--disassemble-symbols={','.join(symbols)}", str(binary)]]


# The Miri runtime's own counters.


def runtime_counts(ctx, workload, input_path):
    """The counters of the Miri port's counted build, which keeps the hooks
    the timed release build leaves out."""
    port = run.counted_build(ctx, workload.name, workload.port("miri"))
    if not isinstance(port, run.Built):
        return {"unavailable": port}
    env = run.child_environment(ctx.work_dir, {"MIRI_ALLOC_COUNT": "1", "MIRI_RC_STATS": "1"})
    done = subprocess.run([*port.run_words, str(input_path)], env=env, capture_output=True, text=True, check=False)
    return {"allocation_count": run.allocation_count(done.stderr), **run.runtime_counters(done.stderr)}


# The bundle.


# Which build of the ports each artifact of the bundle came from. The profile
# build is the charter's build line with `--profile` in place of `--release`
# for Miri and the one release build for every other language; the release
# build is the one that was timed; the counted build is Miri's without
# `--release`, which keeps the runtime's counters.
ARTIFACT_BUILDS = (
    ("profile.<language>.txt", "profile", "the profiler's own report of a hot run"),
    ("folded.<language>.txt", "profile", "its stacks, folded"),
    ("flamegraph.<language>.svg", "profile", "its stacks, drawn"),
    ("top.<language>.json", "profile", "its top self-time symbols"),
    ("counters.json", "release", "hardware counters of a whole run"),
    ("miri.mir", "release", "the Miri port's MIR"),
    ("miri.clif", "release", "the Miri port's CLIF"),
    ("hot.clif", "release", "the CLIF of the hottest Miri functions the profile names"),
    ("hot.asm", "release", "their machine code, deliberately the timed code"),
    ("runtime.json", "counted", "the Miri runtime's allocation and reference counts"),
)


def artifact_builds_markdown():
    lines = ["## Builds", "", "| Artifact | Build | What it is |", "|---|---|---|"]
    lines += [f"| `{artifact}` | {build} | {what} |" for artifact, build, what in ARTIFACT_BUILDS]
    lines += [
        "",
        "The profile build is Miri's `--profile` build, the release code with a frame pointer kept in every function, "
        "and every other language's release build; release is the build that was timed; counted is Miri's build "
        "without `--release`, which keeps the runtime's counters.",
        "",
    ]
    return lines


def diagnosis_markdown(workload, size, summary, profiles, counter_data, runtime):
    """The bundle's summary. `profiles` maps a language to its top symbols, or
    to the reason it has no profile."""
    lines = [f"# {workload.name} at size {size}: diagnosis", "", f"Feature tags: {', '.join(workload.manifest.get('tags', []))}.", ""]
    lines += ["| Language | Median wall (ms) | × C | Max RSS (MB) |", "|---|---|---|---|"]
    for cell in summary["cells"]:
        if cell["workload"] == workload.name and cell["size"] == size and cell["mode"] == "cold" and "metrics" in cell:
            wall = cell["metrics"]["process_wall_ns"]["median"] / 1e6
            vs_c = ((cell.get("vs_c") or {}).get("time") or {}).get("value")
            rss = cell["metrics"]["max_rss_bytes"]["median"] / 2**20
            lines.append(f"| {cell['language']} | {wall:.1f} | {vs_c:.2f} | {rss:.1f} |" if vs_c else f"| {cell['language']} | {wall:.1f} | — | {rss:.1f} |")
    lines += ["", *artifact_builds_markdown(), "## Top self-time symbols", ""]
    for language, top in profiles.items():
        if isinstance(top, str):
            lines.append(f"**{language}**: unavailable: {top}")
        else:
            lines.append(f"**{language}** (profile build): " + ", ".join(f"`{t['symbol']}` {t['share']:.0%}" for t in top[:8]))
        lines.append("")
    lines += ["## Counters (release build)", "", "```json", json.dumps(counter_data, indent=1), "```", ""]
    lines += ["## Miri runtime (counted build)", "", "```json", json.dumps(runtime, indent=1), "```", ""]
    lines += ["## Verdict", "", "Hypothesis: ", "", "Task: ", ""]
    return "\n".join(lines)


def diagnose_cell(ctx, summary, workload, size, out_dir):
    out_dir.mkdir(parents=True, exist_ok=True)
    input_path = run.input_file(ctx, workload, size)
    ports = build_ports(ctx, workload, ctx.work_dir)
    env = run.child_environment(ctx.work_dir)
    (out_dir / "counters.json").write_text(json.dumps(counter_data := counters(ports.timed, input_path, env), indent=1) + "\n")
    profiles = {}
    for language, port in ports.sampled.items():
        profiles[language] = profile_artifacts(port, input_path, env, ctx.work_dir, out_dir / language, f"{workload.name} {size} {language}")
    miri_top = profiles.get("miri")
    copy_listings(ports.listing, out_dir, hot_miri_functions(miri_top if isinstance(miri_top, list) else []))
    runtime = runtime_counts(ctx, workload, input_path) if "miri" in ports.timed else {}
    (out_dir / "runtime.json").write_text(json.dumps(runtime, indent=1) + "\n")
    (out_dir / "DIAGNOSIS.md").write_text(diagnosis_markdown(workload, size, summary, profiles, counter_data, runtime))
    scrub_bundle(out_dir, ctx.work_dir)


def profile_artifacts(port, input_path, env, work_dir, stem, title):
    """Sample one port's profile build and write its profile, folded stacks,
    flamegraph and top symbols beside `stem`, whose name is the language.
    Returns the top symbols, or the reason there is no profile, which
    `profile.<language>.txt` then carries."""
    out_dir, language = stem.parent, stem.name
    sampled = sample_profile(port, input_path, env, work_dir) if isinstance(port, run.Built) else f"the profile build of the port: {port}"
    if not isinstance(sampled, Sampled):
        (out_dir / f"profile.{language}.txt").write_text(f"unavailable: {sampled}\n")
        return str(sampled)
    top = top_symbols(sampled.folded)
    (out_dir / f"profile.{language}.txt").write_text(sampled.report)
    (out_dir / f"folded.{language}.txt").write_text("".join(f"{k} {v}\n" for k, v in sorted(sampled.folded.items())))
    (out_dir / f"flamegraph.{language}.svg").write_text(flamegraph_svg(sampled.folded, title))
    (out_dir / f"top.{language}.json").write_text(json.dumps(top, indent=1) + "\n")
    return top


def copy_listings(miri_listing, out_dir, hot):
    mir = Path(f"{miri_listing}.mir")
    clif = Path(f"{miri_listing}.clif")
    if mir.is_file():
        shutil.copy(mir, out_dir / "miri.mir")
    if clif.is_file():
        shutil.copy(clif, out_dir / "miri.clif")
        sections = listing_sections(clif.read_text(), "; ")
        (out_dir / "hot.clif").write_text("".join(sections.get(name, f"; {name}: not in the listing\n") for name in hot))
    (out_dir / "hot.asm").write_text(disassembly(miri_listing, hot) if hot else "no Miri function among the top symbols\n")


def diagnose(run_dir, only=None, miri=None):
    run_dir = Path(run_dir)
    summary = json.loads((run_dir / "summary.json").read_text())
    args = run.parse_args(["--allow-dirty"] + (["--miri", miri] if miri else []))
    compiler, problem = run.built_compiler(args)
    if problem:
        raise SystemExit(f"refusing to diagnose: {problem}")
    work = Path(run.tempfile.mkdtemp(prefix="miri-diagnose-"))
    ctx = run.Context(args, run.load_charter(), run.charter_hash(), summary["run_id"], work, compiler)
    workloads = {w.name: w for w in run.load_workloads(None)}
    cells = lost_cells(summary, only)
    for name, size in cells:
        diagnose_cell(ctx, summary, workloads[name], size, run_dir / "diagnosis" / f"{name}-{size}")
    shutil.rmtree(work, ignore_errors=True)
    return cells


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("run_dir", help="a results directory with a summary.json")
    parser.add_argument("--cell", help="one workload and size, as workload/size")
    parser.add_argument("--miri", help="the compiler to use (default: this tree's, rebuilt)")
    args = parser.parse_args(argv)
    cells = diagnose(args.run_dir, args.cell, args.miri)
    print(f"diagnosed {len(cells)} cells: " + ", ".join(f"{w}/{s}" for w, s in cells))


if __name__ == "__main__":
    main()
