# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Draw the benchmark charts from a summary, and nothing else.

    python3 bench/plot.py <run>/summary.json <out-dir> [--size m] [--history bench/results/]

Writes, each as SVG and as PNG at twice the size:

- `time-vs-c`: every language's wall time and hot time per workload, as a
  ratio to C on a log₂ axis, C drawn as the line at 1;
- `memory-vs-c`: peak memory as a ratio to C, and net memory (above the
  runtime floor) as a ratio to Rust;
- `time-memory-scatter`: each language's geometric-mean time ratio against
  its geometric-mean memory ratio;
- `miri-history`: Miri's time ratio to C per workload and its geometric
  mean, one point per run in version order (with `--history`, such as the
  points `bench/history.py` writes), footnoting every release whose
  optimisation level did not reach the code generator.

and `where-miri-lost.md`, the table of every Miri cell past the charter's
diagnosis thresholds with the mechanism its diagnosis names.

Every number comes from `summary.json` (schema v1); the charts compute no
ratio of their own. A cell without a measurement is drawn as absent and
labelled, a noisy one hatched, never as zero. The SVG output is
deterministic, so a fresh plot can be compared with a committed one. Needs
matplotlib (bench/requirements.txt).
"""

import argparse
import json
import math
import re
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402
from matplotlib.patches import Patch  # noqa: E402
from matplotlib.ticker import FuncFormatter, LogLocator  # noqa: E402

from summarize import language_label  # noqa: E402

# The categorical palette, in fixed order (validated: every adjacent pair is
# apart under colour-vision deficiency; three slots sit below 3:1 on the
# surface, so every chart carries a legend and the markdown table is its
# table view). Colour follows the language, never its rank.
LANGUAGE_COLORS = {
    "miri": "#2a78d6",
    "c": "#eb6834",
    "cpp": "#1baf7a",
    "rust": "#eda100",
    "go": "#e87ba4",
    "python": "#008300",
    "typescript": "#6250d6",
}
SURFACE = "#fcfcfb"
INK = "#0b0b0b"
SECONDARY_INK = "#52514e"
MUTED = "#898781"
GRID = "#e1e0d9"
BASELINE = "#c3c2b7"
UNMEASURED = {"mismatch", "timeout", "unsupported", "build-failed", "toolchain-missing"}


def ratio_axis(axis):
    """A log₂ ratio axis labelled as plain multipliers (0.5×, 1×, 1.5×, 2×)."""
    axis.set_major_locator(LogLocator(base=2, subs=(1.0, 1.5)))
    axis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:g}×"))
    axis.set_minor_formatter(FuncFormatter(lambda v, _: ""))


def style():
    plt.rcParams.update(
        {
            "svg.hashsalt": "miri-bench",
            "svg.fonttype": "none",
            "font.family": "sans-serif",
            "font.sans-serif": ["DejaVu Sans"],
            "font.size": 9,
            "axes.facecolor": SURFACE,
            "figure.facecolor": SURFACE,
            "axes.edgecolor": BASELINE,
            "axes.labelcolor": SECONDARY_INK,
            "xtick.color": SECONDARY_INK,
            "ytick.color": SECONDARY_INK,
            "text.color": INK,
            "axes.grid": True,
            "grid.color": GRID,
            "grid.linewidth": 0.6,
            "axes.spines.top": False,
            "axes.spines.right": False,
        }
    )


def footer(fig, summary):
    """Provenance on every chart, so a screenshot cannot lose it."""
    p = summary.get("provenance", {})
    label = f"{summary['label'].upper()} · " if summary.get("label") else ""
    text = (
        f"{label}profile {summary['machine_profile']} · {p.get('cpu_model', 'unknown CPU')} · "
        f"{p.get('miri_version', 'unknown Miri')} · charter v{summary['charter_version']} · run {summary['run_id']}"
    )
    fig.text(0.01, 0.01, text, fontsize=7, color=MUTED, ha="left", va="bottom")


def save(fig, out_dir, name):
    fig.savefig(out_dir / f"{name}.svg", metadata={"Date": None, "Creator": None})
    fig.savefig(out_dir / f"{name}.png", dpi=200, metadata={"Software": None})
    plt.close(fig)


def cells_at(summary, size, mode):
    return {(c["workload"], c["language"]): c for c in summary["cells"] if c["size"] == size and c["mode"] == mode and c["variant"] == "default"}


def present_languages(summary):
    seen = {c["language"] for c in summary["cells"]}
    return [lang for lang in LANGUAGE_COLORS if lang in seen]


def ratio_of(cell, path):
    value = cell
    for key in path:
        value = (value or {}).get(key)
    if isinstance(value, dict):
        value = value.get("value")
    return value


def grouped_bars(ax, summary, size, mode, path, title):
    """Bars per workload, one per language, as a ratio on a log₂ axis; an
    unmeasured cell is labelled where its bar would be, a noisy one hatched."""
    cells = cells_at(summary, size, mode)
    workloads = sorted({w for w, _ in cells})
    languages = present_languages(summary)
    width = 0.8 / max(1, len(languages))
    for li, language in enumerate(languages):
        for wi, workload in enumerate(workloads):
            x = wi - 0.4 + width * (li + 0.5)
            cell = cells.get((workload, language))
            value = ratio_of(cell, path) if cell else None
            if cell is None or cell.get("status") in UNMEASURED or value is None or cell.get("reproduced") is False:
                ax.text(x, 1.0, absent_label(cell), rotation=90, fontsize=6, color=MUTED, ha="center", va="bottom")
                continue
            # A ratio's bar grows from 1, the reference, so a faster cell
            # points down and a slower one up.
            ax.bar(
                x,
                value - 1.0,
                width=width,
                bottom=1.0,
                color=LANGUAGE_COLORS[language],
                edgecolor=SURFACE,
                linewidth=1.0,
                hatch="////" if cell.get("status") == "noisy" else None,
            )
    ax.set_yscale("log", base=2)
    ratio_axis(ax.yaxis)
    ax.set_axisbelow(True)
    ax.axhline(1.0, color=INK, linewidth=1.0)
    ax.set_xticks(range(len(workloads)))
    ax.set_xticklabels(workloads, rotation=30, ha="right")
    ax.set_title(title, loc="left", fontsize=10)
    ax.grid(axis="x", visible=False)


def absent_label(cell):
    """Why a cell has no bar: it was not measured, or its memory above the
    runtime floor is too small to be a number."""
    if cell is None:
        return "n/a"
    if cell.get("reproduced") is False:
        return "unreproduced"
    if cell.get("status") in UNMEASURED:
        return cell["status"]
    if (cell.get("metrics", {}).get("net_rss_bytes") or {}).get("at_or_below_floor"):
        return "≤ floor"
    return "n/a"


def legend_handles(languages, extra=True):
    handles = [Patch(facecolor=LANGUAGE_COLORS[lang], label=language_label(lang)) for lang in languages]
    if extra:
        handles.append(Patch(facecolor=SURFACE, edgecolor=MUTED, hatch="////", label="noisy (CV over the charter's threshold)"))
        handles.append(Patch(facecolor=SURFACE, edgecolor=SURFACE, label="label in place of a bar: not measured, or ≤ floor"))
    return handles


def time_chart(summary, out_dir, size):
    fig, axes = plt.subplots(2, 1, figsize=(11, 8))
    grouped_bars(axes[0], summary, size, "cold", ("vs_c", "time"), f"Wall time, start-up included, × C (size {size.upper()})")
    grouped_bars(axes[1], summary, size, "hot", ("vs_c", "time"), f"Hot time, after warm-up, × C (size {size.upper()})")
    fig.legend(handles=legend_handles(present_languages(summary)), loc="upper right", fontsize=8, frameon=False)
    footer(fig, summary)
    fig.tight_layout(rect=(0, 0.03, 0.76, 1))
    save(fig, out_dir, "time-vs-c")


def memory_chart(summary, out_dir, size):
    fig, axes = plt.subplots(2, 1, figsize=(11, 8))
    grouped_bars(axes[0], summary, size, "cold", ("vs_c", "max_rss"), f"Peak memory, runtime included, × C (size {size.upper()})")
    grouped_bars(axes[1], summary, size, "cold", ("vs_rust", "net_rss"), f"Memory above the runtime floor, × Rust (size {size.upper()})")
    fig.legend(handles=legend_handles(present_languages(summary)), loc="upper right", fontsize=8, frameon=False)
    footer(fig, summary)
    fig.tight_layout(rect=(0, 0.03, 0.76, 1))
    save(fig, out_dir, "memory-vs-c")


def checks_chart(summary, out_dir, size):
    """Miri and Rust against Rust built with Miri's checks: Miri's bar is what
    the compiler costs beyond the checks, Rust's what the checks cost. Drawn
    only when that arm was measured."""
    if not any(c["language"] == "rust-checked" and "metrics" in c for c in summary["cells"]):
        return
    fig, axes = plt.subplots(2, 1, figsize=(11, 8))
    for ax, mode, label in ((axes[0], "cold", "Wall time"), (axes[1], "hot", "Hot time")):
        cells = {k: c for k, c in cells_at(summary, size, mode).items() if k[1] in ("miri", "rust")}
        grouped_bars(ax, {"cells": list(cells.values())}, size, mode, ("vs_rust_checked", "time"), f"{label} × Rust with Miri's checks (size {size.upper()})")
    fig.legend(handles=legend_handles(["miri", "rust"]), loc="upper right", fontsize=8, frameon=False)
    footer(fig, summary)
    fig.tight_layout(rect=(0, 0.03, 0.76, 1))
    save(fig, out_dir, "checks-cost")


def scatter_chart(summary, out_dir, size):
    """One point per language, every point labelled with its name, so the
    chart never asks colour alone to tell seven languages apart."""
    fig, ax = plt.subplots(figsize=(7, 6))
    rows = [r for r in summary["aggregates"] if r["size"] == size and r["mode"] == "cold" and r["tag"] == "all" and r["variant"] == "default"]
    for row in rows:
        if row["language"] not in LANGUAGE_COLORS or "geomean_max_rss_vs_c" not in row:
            continue
        x, y = row["geomean_vs_c"], row["geomean_max_rss_vs_c"]
        ax.scatter([x], [y], s=64, color=LANGUAGE_COLORS[row["language"]], edgecolor=SURFACE, linewidth=2, zorder=3)
        ax.annotate(f"{row['language']} ({row['coverage']})", (x, y), xytext=(6, 4), textcoords="offset points", fontsize=8, color=SECONDARY_INK)
    ax.set_xscale("log", base=2)
    ax.set_yscale("log", base=2)
    ratio_axis(ax.xaxis)
    ratio_axis(ax.yaxis)
    ax.axvline(1.0, color=BASELINE, linewidth=1.0)
    ax.axhline(1.0, color=BASELINE, linewidth=1.0)
    ax.set_xlabel("geometric-mean wall time × C")
    ax.set_ylabel("geometric-mean peak memory × C")
    ax.set_title(f"Time against memory, size {size.upper()} (workloads counted in brackets)", loc="left", fontsize=10)
    footer(fig, summary)
    fig.tight_layout(rect=(0, 0.04, 1, 1))
    save(fig, out_dir, "time-memory-scatter")


def history_chart(series, summary, out_dir, size):
    """Miri's time ratio to C per workload at `size`, one point per run in
    version order, with the geometric mean over every workload drawn in
    Miri's colour and labelled with how many workloads it covers. A run whose
    compiler's optimisation level did not reach the code generator, as the
    run recorded it, is named in the footnote."""
    fig, ax = plt.subplots(figsize=(11, 5.5))
    miri = [s for s in series if s["language"] == "miri" and s["mode"] == "cold" and s["size"] == size and s["variant"] == "default"]
    draw_history_lines(ax, miri)
    points = next((s["points"] for s in series if s.get("aggregate") == "runs"), None) or history_points(miri)
    label_unbuilt_points(ax, points)
    ax.set_xticks([p["index"] for p in points])
    ax.set_xticklabels([p["label"] for p in points], rotation=30, ha="right")
    ax.set_xlim(-0.5, max((p["index"] for p in points), default=0) + 1.0)
    ax.set_yscale("log", base=2)
    ratio_axis(ax.yaxis)
    ax.axhline(1.0, color=INK, linewidth=1.0)
    ax.set_title(f"Miri wall time × C by version, size {size.upper()} (log₂; bold: geometric mean, workloads in brackets)", loc="left", fontsize=10)
    if note := unoptimized_note(points):
        fig.text(0.01, 0.045, note, fontsize=7, color=SECONDARY_INK, ha="left", va="bottom")
    footer(fig, summary)
    fig.tight_layout(rect=(0, 0.08, 1, 1))
    save(fig, out_dir, "miri-history")


def draw_history_lines(ax, miri):
    """Each workload's line in grey, labelled at its last point, and the
    geometric mean in Miri's colour, each point labelled with its value and
    the workloads it covers."""
    for s in miri:
        xs = [p["index"] for p in s["points"]]
        ys = [p["vs_c"] for p in s["points"]]
        if s.get("aggregate"):
            ax.plot(xs, ys, linewidth=2.5, marker="o", markersize=6, color=LANGUAGE_COLORS["miri"], zorder=3)
            for x, p in zip(xs, s["points"]):
                ax.annotate(f"{p['vs_c']:.2f}× ({p['coverage']})", (x, p["vs_c"]), xytext=(6, -10), textcoords="offset points", fontsize=8, color=LANGUAGE_COLORS["miri"])
        else:
            ax.plot(xs, ys, linewidth=1.2, marker="o", markersize=3, color=MUTED)
            ax.annotate(s["workload"], (xs[-1], ys[-1]), xytext=(4, 0), textcoords="offset points", fontsize=7, color=SECONDARY_INK)


def label_unbuilt_points(ax, points):
    """A run where Miri built no port has no ratio to draw: say so at its
    place on the line instead of leaving a gap."""
    for p in points:
        miri = (p.get("coverage") or {}).get("miri", {})
        if not miri.get("measured") and miri.get("unsupported"):
            label = f"no port compiles\n({miri['unsupported']} unsupported)"
            ax.annotate(label, (p["index"], 1.0), xytext=(0, 8), textcoords="offset points", fontsize=7, color=MUTED, ha="center")


def history_points(series):
    """Every point on the version line once, in order: its index and label."""
    seen = {p["index"]: p for s in series for p in s["points"]}
    return [seen[i] for i in sorted(seen)]


def unoptimized_note(points):
    """The footnote naming the points whose runs recorded that the compiler's
    optimisation level did not reach its code generator, or None."""
    labels = [p["label"] for p in points if p.get("codegen_optimized") is False]
    if not labels:
        return None
    return (
        f"{', '.join(labels)}: measured as released, when --release and --opt-level did not yet reach the code generator, "
        "so Cranelift compiled every program unoptimised."
    )


def hypothesis(diagnosis_dir, workload, size):
    path = Path(diagnosis_dir) / f"{workload}-{size}" / "DIAGNOSIS.md"
    if not path.is_file():
        return "not yet diagnosed", ""
    text = path.read_text()
    found = re.search(r"^Hypothesis:\s*(.*)$", text, re.MULTILINE)
    task = re.search(r"^Task:\s*(.*)$", text, re.MULTILINE)
    return (found.group(1).strip() if found and found.group(1).strip() else "not yet named"), (task.group(1).strip() if task else "")


def lost_table(summary, out_dir, diagnosis_dir):
    lines = [
        "| Workload | Size | Mode | Time × C | Net memory × Rust | Mechanism | Work |",
        "|---|---|---|---|---|---|---|",
    ]
    for cell in summary["cells"]:
        if cell["language"] != "miri" or not cell.get("diagnose"):
            continue
        time = ratio_of(cell, ("vs_c", "time"))
        net = ratio_of(cell, ("vs_rust", "net_rss"))
        mechanism, task = hypothesis(diagnosis_dir, cell["workload"], cell["size"])
        lines.append(
            f"| {cell['workload']} | {cell['size'].upper()} | {cell['mode']} | {time:.2f} | {'—' if net is None else f'{net:.2f}'} | {mechanism} | {task} |"
            if time is not None
            else f"| {cell['workload']} | {cell['size'].upper()} | {cell['mode']} | — | {'—' if net is None else f'{net:.2f}'} | {mechanism} | {task} |"
        )
    (out_dir / "where-miri-lost.md").write_text("\n".join(lines) + "\n")


def chosen_size(summary, size):
    sizes = {c["size"] for c in summary["cells"]}
    if size:
        return size
    return next((s for s in ("m", "s", "l") if s in sizes), "s")


def plot(summary, out_dir, size=None, history=None, diagnosis_dir=None):
    style()
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    size = chosen_size(summary, size)
    time_chart(summary, out_dir, size)
    memory_chart(summary, out_dir, size)
    scatter_chart(summary, out_dir, size)
    checks_chart(summary, out_dir, size)
    if history is not None:
        history_chart(history, summary, out_dir, size)
    lost_table(summary, out_dir, diagnosis_dir or out_dir / "diagnosis")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("summary")
    parser.add_argument("out_dir")
    parser.add_argument("--size", choices=["s", "m", "l"])
    parser.add_argument("--history", help="a directory of runs, for the version chart")
    args = parser.parse_args(argv)
    summary_path = Path(args.summary)
    summary = json.loads(summary_path.read_text())
    history = None
    if args.history:
        import summarize

        summaries = [json.loads(p.read_text()) for p in sorted(Path(args.history).glob("*/summary.json"))]
        history = summarize.history([s for s in summaries if s["machine_profile"] == summary["machine_profile"]])
    plot(summary, args.out_dir, args.size, history, summary_path.parent / "diagnosis")
    print(f"charts written to {args.out_dir}")


if __name__ == "__main__":
    main()
