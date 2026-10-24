# CPU benchmark suite

How fast compiled Miri programs run and how much memory they hold, next to the
languages people would otherwise write the same programs in. The rules are in
[`CHARTER.md`](CHARTER.md), frozen before the first measured run, and its
machine-readable twin [`charter.toml`](charter.toml).

| Path | What it is |
|---|---|
| `workloads/` | One directory per workload: a port per language and its manifest. See [`workloads/README.md`](workloads/README.md). |
| `c/table.h` | The structures the C ports share, which C's standard library lacks. |
| `hello/` | A hello-world per language, for the runtime floor (`floor_rss_bytes`, `startup_ns`). |
| `machine/` | The machine profiles, and the scripts that prepare and check a machine and measure its noise. |
| `toolchains.toml`, `Dockerfile` | The pinned toolchain versions, and an image that installs exactly those. |
| `run.py` | The runner. |
| `summarize.py` | Statistics, ratios and verdicts, from the runner's records. |
| `schema/summary.v1.json` | The schema of `summary.json`, which charts and the website read and nothing else. |
| `diagnose.py` | The evidence for every cell Miri loses. |
| `diagnosis/CATALOG.md` | Every mechanism a loss has been traced to, and where its work stands. |
| `results/` | What the runner writes. Ignored by git: a run is output, not source. |

## Running it

```bash
make bench-smoke                                    # size S; Miri, C and Rust; five measured runs
make bench-full                                     # every workload, language and size
python3 bench/run.py --only nbody --langs miri,c --sizes s,m --runs 5
```

The runner rebuilds this tree's release compiler and runtime libraries with
`make release` first, so a stale binary is never measured (`--miri <path>`
measures a given compiler instead). It refuses to start from a tree with
uncommitted changes unless `--allow-dirty` is given, and refuses a port whose
review line does not match its text.

Before timing anything it builds every port with its charter build line and
checks it prints its manifest's checksum: a port that does not is recorded as
`mismatch` and never timed. It compiles a fixed probe at `--opt-level 0` and
`3` and refuses to run if the two emit the same machine code. Then, for every
workload and size, it runs each language's warm-up and measured runs
interleaved, one language after another in every round, so drift over the run
lands on all of them alike. Each run is a fresh process, executed directly
with no shell, in an environment holding only `PATH`, `HOME`,
`PYTHONHASHSEED=0` and a pinned `DENO_DIR`. Wall time is taken around fork and
exit, and CPU time and peak memory come from the child's `wait4` resource usage.
A noisy cell is re-run from its warm-up as the charter allows, and every
attempt is kept. Sizes S and M also run in hot mode. Each Miri cell gets one
extra, untimed run with the runtime's allocation counter on, from a build of
the same line without `--release`: a release build leaves the counters out.

## What it writes

`results/<date>-<profile>/` holds `env.json`, the machine, operating system
and toolchain versions, and `results.jsonl`, one JSON object per measured
run, schema version 1. Every record carries the charter version and hash, the
run id, the machine profile, the runner commit, the Miri version, the
workload's manifest hash, the language, variant, size and mode, and
`process_wall_ns`, `user_ns`, `sys_ns`, `max_rss_bytes`, `hot_ns` (with every
`hot_ns_samples` line), `floor_rss_bytes`, `startup_ns`, `binary_bytes`,
`exit_code` and `status`: `ok` or `timeout`. A cell that produced no
measurements has one record saying why: `mismatch`, `build-failed`,
`toolchain-missing` or `unsupported`, the last with the manifest's reason
when the port is blocked. Miri's `alloc-count` records carry `allocations`.
A results directory is never overwritten; a second run on the same day gets a
new one.

## Summarizing and comparing

```bash
python3 bench/summarize.py bench/results/<run>/      # writes summary.json and summary.md there
python3 bench/summarize.py compare <A>/summary.json <B>/summary.json
python3 bench/summarize.py history bench/results/    # each cell's ratio to C, run by run
```

`summarize.py` applies the charter's statistics to each cell's last attempt:
median, minimum, the upper quantile by nearest rank, the median absolute
deviation and the coefficient of variation of every metric, and a `noisy`
flag past the charter's threshold. A noisy cell is published flagged, never
dropped. It adds each cell's ratio to C (time and peak memory), to Rust (peak
and net memory, net being the peak above the runtime floor, or `≤ floor`), and
to Miri. It marks the Miri cells past the charter's diagnosis thresholds, and
takes the geometric mean of each language's time ratio to C, overall and per
feature tag, with how many workloads it covers, and carries each measured
workload's manifest `notes` into `port_notes`. A cell that was not measured
(`mismatch`, `timeout`, `unsupported` …) keeps its status and never enters a
ratio. It refuses records made under a charter other than the one on disk,
and a run that mixes machine profiles or charter versions.

`compare` gives a verdict per cell, `faster`, `same` or `slower`, by the
charter's rule: the change in ratio to C must exceed the larger of 5 % and
three times the ratio's coefficient of variation. It refuses two runs from
different machine profiles or charter versions. `summary.md` is generated
from `summary.json`; neither is edited by hand.

## Diagnosing a loss

```bash
python3 bench/diagnose.py bench/results/<run>/                 # every cell summary.json marks
python3 bench/diagnose.py bench/results/<run>/ --cell nbody/s  # one workload and size
python3 bench/run.py --full --diagnose                          # what `make bench-full` runs
```

Every Miri cell past the charter's diagnosis thresholds gets a bundle under
`<run>/diagnosis/<workload>-<size>/`, comparing Miri with C and Rust:
hardware counters (`perf stat` on Linux, and the reason they are unavailable
elsewhere); a sampled profile of a hot run, from the operating system's own
`sample` on macOS, folded into stacks, drawn as a flamegraph and ranked into
its top self-time symbols; the Miri port's MIR and CLIF from
`miri build --emit mir --emit clif`, with the CLIF and machine code of its
hottest functions; and the runtime's allocation counts. `DIAGNOSIS.md` lays
the evidence side by side and leaves the hypothesis and the task empty. The
tool gathers evidence; the engineer names the mechanism, files the work, and
records it in [`diagnosis/CATALOG.md`](diagnosis/CATALOG.md), which the next
diagnosis reads first.

## Charts and the website

```bash
pip install -r bench/requirements.txt                      # matplotlib, pinned
python3 bench/plot.py bench/results/<run>/summary.json out/ [--history bench/results/]
python3 bench/export_site.py                               # newest committed publication run
python3 bench/export_site.py --check                       # what the site-identity test runs
```

`plot.py` draws from `summary.json` alone and computes no ratio of its own:
time and hot time against C, peak memory against C and net memory against
Rust, each language's time against its memory, and Miri's ratio to C run by
run. Each chart is SVG and PNG, carries the profile, CPU, Miri version and
charter version in its footer, labels a cell it has no measurement for
instead of drawing it as zero, and hatches a noisy one. `where-miri-lost.md`
is the table of Miri cells past the diagnosis thresholds, with the mechanism
each `DIAGNOSIS.md` names. The SVG is deterministic.

`export_site.py` copies a publication run to the website checkout at
`../miri-lang.org`: `_data/bench/<miri-version>.json` and `latest.json` (the
medians, ratios, aggregates and provenance of `summary.json`) and the SVGs
under `assets/bench/<miri-version>/`. A dev run is never exported. A
publication run is committed under `bench/results/` by name, and the site is
regenerated from it in the same pass; `tests/bench/site_identity.rs` fails
when the site's copy differs from a fresh export, and skips with a log line
when the sibling checkout or matplotlib is absent.

## Machines

A run names its machine profile with `--profile`, read from
`machine/<profile>.toml`. Only `publication` may produce a published number:
a dedicated Linux x86-64 box, prepared with `sudo machine/setup-linux.sh`
(the performance governor, turbo off, the pinned core's hyper-thread sibling
offline). On it the runner pins every process tree to the profile's core with
`taskset`, writes inputs to a memory-backed directory, and checks the
machine's state before every cell, stopping the run rather than publishing
from a machine that drifted. `machine/check.sh` makes the same checks by hand,
and refuses publication outright on macOS. `dev` is a workstation, for
diagnosis only, and `ci-smoke` a shared CI runner, read through ratios to C
alone.

`python3 bench/machine/noise.py` runs the C reference of one workload many
times and reports the coefficient of variation of its wall time: a
publication box must measure under the charter's `noisy_cv`, and the result
goes into its profile.

`Dockerfile` installs exactly the versions `toolchains.toml` pins, so a reader
can rebuild the environment (`docker build -f bench/Dockerfile -t miri-bench
bench/`). It is a reproducibility artifact; a publication run happens on bare
metal and records that it did.

## Every night, and every week

`.github/workflows/bench-nightly.yml` runs the core compute workloads,
binary-trees and wordfreq at size S on a hosted runner every night, for Miri,
C and Rust. Its absolute times are noise and are never published.
`bench/regress.py` compares each Miri cell's ratio to C (time) and to Rust
(net memory) with the median of the last seven nights. A cell past the
charter's verdict threshold is suspect, and suspect two nights running is a
regression: the job opens an issue for it and closes the issue when the cell
recovers. Each night's `summary.json`, `env.json` and verdicts are kept on
the `bench-history` branch, never on `main`. `make bench-regress` judges a
working-tree change against those nights before it is pushed.

`.github/workflows/bench-weekly.yml` runs the full suite on the publication
machine every week, diagnoses its losses and proposes the run as a pull
request; a person commits publication results after looking at the noise
flags. It stays idle until a self-hosted runner labelled `publication` exists
and the repository variable `BENCH_PUBLICATION_RUNNER` is `true`.
