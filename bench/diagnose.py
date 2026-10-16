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
  drawn, and its top self-time symbols;
- `miri.mir` and `miri.clif`: what the compiler made of the Miri port, and
  `hot.clif` and `hot.asm`: the CLIF and machine code of its hottest functions;
- `runtime.json`: the Miri runtime's allocation counts;
- `DIAGNOSIS.md`: the evidence side by side, with an empty hypothesis and an
  empty task for the engineer to fill. The tool gathers evidence; a person
  names the mechanism, files the work, and adds it to `diagnosis/CATALOG.md`.

Standard library only; the profiler is the operating system's own.
"""

import argparse
import html
import json
import platform
import re
import shutil
import subprocess
import time
from pathlib import Path

import run

LANGUAGES = ("miri", "c", "rust")
PERF_EVENTS = "cycles,instructions,branches,branch-misses,cache-misses,L1-dcache-load-misses"
SAMPLE_SECONDS = 3
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


def build_ports(ctx, workload, work_dir):
    """Each language's built port, keyed by language; Miri's build also writes
    its MIR and CLIF listings beside the executable."""
    built = {}
    for language in LANGUAGES:
        src = workload.port(language)
        if not src.is_file() or workload.blocked(language):
            continue
        result = run.build(ctx, workload.name, language, src, "default")
        if isinstance(result, run.Built):
            built[language] = result
    miri_out = ctx.work_dir / "bin" / f"{workload.name}.listing"
    words = [str(ctx.miri), "build", "--release", "--opt-level", "3", "--emit", "mir", "--emit", "clif", "-o", str(miri_out), str(workload.port("miri"))]
    subprocess.run(words, env=ctx.build_env("miri"), cwd=work_dir, capture_output=True, check=False)
    return built, miri_out


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


def sample_profile(port, input_path, env, work_dir):
    """A sampled call graph of the port's hot run, as the profiler printed
    it, or None when no profiler is available."""
    sampler = shutil.which("sample")
    if sampler is None:
        return None
    child = subprocess.Popen([*port.run_words, str(input_path), "hot"], env=env, cwd=work_dir, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.2)
    out = work_dir / f"sample.{child.pid}.txt"
    subprocess.run([sampler, str(child.pid), str(SAMPLE_SECONDS), "-mayDie", "-file", str(out)], capture_output=True, check=False)
    child.kill()
    child.wait()
    return out.read_text() if out.is_file() else None


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


def hot_miri_functions(top):
    return [entry["symbol"] for entry in top if entry["symbol"].startswith("miri.")][:HOT_FUNCTIONS]


def disassembly(binary, symbols):
    """Machine code of `symbols` in `binary`, through objdump."""
    objdump = shutil.which("objdump")
    if objdump is None:
        return "unavailable: objdump is not installed\n"
    prefix = "_" if platform.system() == "Darwin" else ""
    names = ",".join(prefix + s for s in symbols)
    done = subprocess.run([objdump, "-d", f"--disassemble-symbols={names}", str(binary)], capture_output=True, text=True, check=False)
    # objdump names the binary by its path, which is a scratch directory of
    # this machine's; the bundle names it by the workload instead.
    return done.stdout.replace(str(binary), binary.name) or f"unavailable: {done.stderr.strip()}\n"


# The Miri runtime's own counters.


def runtime_counts(port, input_path, ctx):
    env = run.child_environment(ctx.work_dir, {"MIRI_ALLOC_COUNT": "1"})
    done = subprocess.run([*port.run_words, str(input_path)], env=env, capture_output=True, text=True, check=False)
    line = next((l for l in done.stderr.splitlines() if l.startswith("MIRI_ALLOC_COUNT:")), None)
    return {"allocation_count": run.allocation_count(done.stderr), "line": line}


# The bundle.


def diagnosis_markdown(workload, size, summary, tops, counter_data, runtime):
    lines = [f"# {workload.name} at size {size}: diagnosis", "", f"Feature tags: {', '.join(workload.manifest.get('tags', []))}.", ""]
    lines += ["| Language | Median wall (ms) | × C | Max RSS (MB) |", "|---|---|---|---|"]
    for cell in summary["cells"]:
        if cell["workload"] == workload.name and cell["size"] == size and cell["mode"] == "cold" and "metrics" in cell:
            wall = cell["metrics"]["process_wall_ns"]["median"] / 1e6
            vs_c = ((cell.get("vs_c") or {}).get("time") or {}).get("value")
            rss = cell["metrics"]["max_rss_bytes"]["median"] / 2**20
            lines.append(f"| {cell['language']} | {wall:.1f} | {vs_c:.2f} | {rss:.1f} |" if vs_c else f"| {cell['language']} | {wall:.1f} | — | {rss:.1f} |")
    lines += ["", "## Top self-time symbols", ""]
    for language, top in tops.items():
        lines.append(f"**{language}**: " + ", ".join(f"`{t['symbol']}` {t['share']:.0%}" for t in top[:8]))
        lines.append("")
    lines += ["## Counters", "", "```json", json.dumps(counter_data, indent=1), "```", "", "## Miri runtime", "", "```json", json.dumps(runtime, indent=1), "```", ""]
    lines += ["## Verdict", "", "Hypothesis: ", "", "Task: ", ""]
    return "\n".join(lines)


def diagnose_cell(ctx, summary, workload, size, out_dir):
    out_dir.mkdir(parents=True, exist_ok=True)
    input_path = run.input_file(ctx, workload, size)
    built, miri_listing = build_ports(ctx, workload, ctx.work_dir)
    env = run.child_environment(ctx.work_dir)
    (out_dir / "counters.json").write_text(json.dumps(counter_data := counters(built, input_path, env), indent=1) + "\n")
    tops = {}
    for language, port in built.items():
        profile = sample_profile(port, input_path, env, ctx.work_dir)
        if profile is None:
            (out_dir / f"profile.{language}.txt").write_text("unavailable: no sampling profiler on this machine\n")
            continue
        folded = folded_stacks(call_tree(profile))
        tops[language] = top_symbols(folded)
        (out_dir / f"profile.{language}.txt").write_text(profile)
        (out_dir / f"folded.{language}.txt").write_text("".join(f"{k} {v}\n" for k, v in sorted(folded.items())))
        (out_dir / f"flamegraph.{language}.svg").write_text(flamegraph_svg(folded, f"{workload.name} {size} {language}"))
        (out_dir / f"top.{language}.json").write_text(json.dumps(tops[language], indent=1) + "\n")
    copy_listings(miri_listing, out_dir, hot_miri_functions(tops.get("miri", [])))
    runtime = runtime_counts(built["miri"], input_path, ctx) if "miri" in built else {}
    (out_dir / "runtime.json").write_text(json.dumps(runtime, indent=1) + "\n")
    (out_dir / "DIAGNOSIS.md").write_text(diagnosis_markdown(workload, size, summary, tops, counter_data, runtime))


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
