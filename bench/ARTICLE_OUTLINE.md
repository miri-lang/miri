# Article outline: the preliminary benchmark

This outline is committed before the run it describes, so the article's shape
cannot be fitted to its numbers. It fixes what the article reports, in what
order, and where every number comes from. It makes no claim about who wins: the
article reports what the run measured and, where Miri loses, why.

Every number in the article is generated from the run's `summary.json` (the
session-two summary, made with `summarize.py <two> --reproduces
<one>/summary.json`) by the path written beside it here. A number with no path
does not appear. `<w>` is a workload, `<l>` a language, `<s>` a size, `<m>` a
mode (`cold` or `hot`).

The run is labelled **preliminary** everywhere it appears: the title, every
chart and every table, as `bench/CHARTER.md` section 8 requires of the
`preliminary-vps` profile.

## 1. What was measured, and why these programs

- The workloads, one sentence each, from each `bench/workloads/<w>/workload.toml`
  (`name`, `tags`, `parameter`).
- The languages and how each is built, from `bench/charter.toml`
  `[languages.<l>]`; the toolchain versions from
  `summary.provenance.toolchain_versions`.
- Coverage: `summary.coverage.<l>` (the cells each language ran), with every
  `unsupported` cell named.

## 2. The fairness contract, in five sentences

One sentence each, linking `bench/CHARTER.md`: what a port may and may not do
(section 3), how programs are built, how time and memory are measured from
outside the process, how many runs a cell takes and when a noisy cell is
re-run, and the verdict rule that makes a ratio a win, a loss or a tie.
Charter version and hash from `summary.charter_version` and
`summary.charter_hash`.

## 3. The machine

- CPU, kernel and profile from `summary.provenance` (`cpu_model`, `kernel`,
  `os`) and `summary.machine_profile`.
- Its limits, stated plainly: a shared-vCPU virtual machine with no frequency
  or turbo control, and its measured noise from
  `bench/machine/preliminary-vps.toml` (`measured_noise_cv`).
- What the run did about them: the pinned core (`summary.provenance.pinned_core`),
  inputs in memory, steal time read around every run
  (`summary.max_steal_share`), and the second session: a cell is drawn only
  when it is marked reproduced.

## 4. The headline charts

- Figure 1, time against C per workload at size M:
  `summary.cells[w=<w>, l=<l>, s=m, m=hot].vs_c.time.value`, one bar per
  language; cold mode where a language has no hot mode.
- Figure 2, the geometric means:
  `summary.aggregates[l=<l>, s=<s>, m=<m>, variant=default].geomean_vs_c`, with
  `coverage` beside each.
- Figure 3, memory against C:
  `summary.cells[...].vs_c.max_rss.value` and
  `summary.aggregates[...].geomean_max_rss_vs_c` (`memory_coverage`).
- Figure 4, time against memory (the scatter `plot.py` draws from the same
  two values).

## 5. `hot` against `process` for the runtimes that start up

For Python and TypeScript, the gap between a cold run's
`metrics.process_wall_ns.median` and a hot run's, per workload, with
`startup_ns` beside it: what start-up and warm-up cost, kept apart from the
steady state the other figures show.

## 6. Memory, with the floors shown

Peak resident memory per cell, `metrics.max_rss_bytes.median`, beside the
language's floor `floor_rss_bytes` (a hello-world built the same way) and the
net figure `metrics.net_rss_bytes.value`; a cell at or below its floor
(`net_rss_bytes.at_or_below_floor`) says so instead of showing a ratio.

## 7. Where Miri lost, and why

One paragraph per cell whose verdict against C or Rust is a loss
(`summary.cells[l=miri].vs_c` / `vs_rust` under the charter's verdict rule),
each naming the mechanism from that cell's `DIAGNOSIS.md` in the run's
diagnosis bundle and its entry in `bench/diagnosis/CATALOG.md`. A loss whose
mechanism is not yet known says so; none is left without a paragraph. The
price of the run-time checks is stated from the `rust-checked` cells:
`summary.cells[l=rust-checked].vs_rust`.

## 8. What changed since the last version

The history chart (`miri-history`, from `bench/history.py` and `summarize.py
history`): each release as measured, with the footnote the chart generates
for releases whose optimisation level never reached the code generator.

## 9. How to reproduce it

The image (`docker build -f bench/Dockerfile -t miri-bench bench/`), the
commands of both sessions, the commit (`summary.provenance` `miri_sha`), the
results directory as committed under `bench/results/`, and the charter
version.
