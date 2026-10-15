# Benchmark charter

This document is the contract the CPU benchmark suite runs under. It was
written, reviewed and committed before the first measured run. Every rule here
exists so that a reader who distrusts the result can check how it was produced,
and every rule was fixed before any number existed that it could have been
fitted to. The pre-registered claims of the agent benchmark
(`evals/field/CLAIMS.md`) are the precedent: rules first, results after, and a
change to a rule is dated and explained, never made quietly.

The suite answers two questions about compiled Miri programs: how fast they run
and how much memory they hold, against the languages people would otherwise
write the same program in. It is also a finder. A cell Miri loses is published
as a loss, diagnosed down to a mechanism, and filed as work on the compiler.

`charter.toml` beside this file is its machine-readable twin. The runner, the
statistics and the charts read every parameter from there and from nowhere
else. Every value is listed with its meaning in the Parameters table at the end
of this document, and the prose refers to values by key rather than restating
them. `tests/bench/charter.rs` fails the build when the two files disagree on a
number, a build line, a tier or a key.

**Charter version: 1.**

## 1. Identity of a result

Every result record carries:

- `charter_version`, the integer above;
- `charter_hash`, the lowercase hex SHA-256 of the bytes of `CHARTER.md`
  followed immediately by the bytes of `charter.toml`, both as committed;
- `runner_commit`, the repository commit the runner and the ports were taken
  from;
- `manifest_sha256`, the SHA-256 of the workload's manifest, which holds its
  size parameters (section 7);
- `image_digest`, the digest of the container image the toolchains came from;
- the machine profile (section 8), the CPU model, the operating system and
  kernel version;
- per language: the version string its toolchain reports, whether the binary is
  linked statically or dynamically, the C or C++ standard library it links, and
  the allocator its runtime uses; for Miri, the build profile of the runtime
  library linked into the program.

The statistics stage refuses a record whose `charter_hash` does not match the
charter it names. Changing the runner, a port or a manifest therefore shows in
every record it produces, and changing the rules shows in the hash. Results
under different charter versions are never drawn on the same chart
(section 10).

## 2. Languages and tiers

| Tier | Languages | Role |
|---|---|---|
| Subject | Miri `miri` | What is measured. |
| Headline | C `c`, C++ `cpp`, Rust `rust`, Go `go`, Python `python`, TypeScript on Deno `typescript` | Mandatory. Every published chart shows all six. A run missing one of them is not a publication run. |
| Diagnostic | Swift `swift`, Koka `koka` | Opt-in, skipped cleanly when the toolchain is absent. Swift shares Miri's memory model (reference counting with copy-on-write collections); Koka is the reference implementation of Perceus, the reference-counting scheme Miri uses, and tells us whether Miri's Perceus does what the original does. They explain results; they are not the headline. |
| Optional | PyPy `pypy` | A second Python arm, shown beside CPython and never in its place. CPython is the Python people run. |

Python means CPython in its default build: with the global interpreter lock and
with the experimental JIT off (`PYTHON_JIT` unset).

C is the reference every time is measured against (section 6). It has no
standard containers, so its ports are the one exception to checklist item 3:
they implement the plain structure the algorithm needs (a growable array, a
chained hash table with a textbook string hash), written once, shared by every C
workload, and reviewed like any port. That is also why the memory reference is
Rust rather than C (section 9): Rust frees deterministically, as Miri does, and
uses standard containers, so a memory gap against Rust is the language's and not
the port author's.

## 3. The port checklist

Each workload is written once per language. A port is admitted only if it holds
to every item:

1. **Same algorithm.** The same data structures in the same asymptotic shape,
   the same traversal order where order changes the work done. A port that is
   faster because it computes something different is not a port.
2. **Same I/O.** Read the whole input once, from the file named by the first
   argument. Write the checksum once, to stdout, as a single line. Nothing else
   goes to stdout. stdout must be byte-identical across every language for a
   given workload and size; a port that differs fails the identity gate and is
   never timed.
3. **Standard containers only.** The language's standard library and nothing
   else: no third-party crates, modules or packages, no hand-rolled hash
   functions or hash tables standing in for the standard ones (C excepted, as
   section 2 says).
4. **No unsafe shortcuts.** No SIMD intrinsics, no `unsafe` blocks, no
   unchecked indexing, no arena or pool allocators, no custom allocators.
   Every language runs on its default allocator and its default memory
   manager.
5. **Single-threaded.** One thread of program work. A runtime's own service
   threads (a garbage collector's, a JIT's) are part of that runtime and stay.
6. **No flags beyond the build line.** The build and run lines in section 4 are
   the only ones used. No profile-guided optimisation, no link-time
   optimisation unless the line says so.
7. **Same sizing decisions.** A container is pre-sized only where the algorithm
   knows its final size before filling it, and then in every port whose
   language can; otherwise in none. The choice is the same across ports and is
   checked in review.
8. **Idiomatic, not golfed.** The port is what a competent programmer in that
   language would write for a reader: no micro-optimisation a code review would
   reject, in any language including Miri.

Each port carries, near its top, a line of the form

```
// port-checklist: reviewed <YYYY-MM-DD> by <reviewer> sha256=<hex>
```

using the language's line comment (`checklist_comment` in `charter.toml`).
`<hex>` is the SHA-256 of the port's file with that one line removed, so the
review is bound to the exact text reviewed: the runner recomputes it and refuses
a port whose hash does not match, or that has no such line. The reviewer is not
the port's author.

A Miri port follows the same list. When idiomatic Miri is slow because the
compiler does something badly, the port stays idiomatic and the weakness is
filed against the compiler. Rewriting the Miri port around a compiler weakness
is forbidden: it would hide exactly what the suite exists to find.

A port changed after a result for its workload exists — a better C port
submitted by a reader, a fix — gets a new review line and takes effect from the
next run, in which every language of that workload is re-run.

## 4. Build and run lines

`{src}` is the port's source file and `{out}` the executable the build writes.
The runner appends the input path, and in hot mode the word `hot` (section 5),
to the run line. Every build and run command is executed directly with
`execve`, never through a shell; a leading `NAME=value` word is set in the
child's environment, not interpreted by a shell. So `wait4` measures the
program, never a shell around it.

| Language | Build | Native build | Run |
|---|---|---|---|
| `miri` | `miri build --release --opt-level 3 -o {out} {src}` | n/a | `{out}` |
| `c` | `clang -std=c17 -O3 -o {out} {src} -lm` | `clang -std=c17 -O3 -march=native -o {out} {src} -lm` | `{out}` |
| `cpp` | `clang++ -std=c++20 -O3 -o {out} {src}` | `clang++ -std=c++20 -O3 -march=native -o {out} {src}` | `{out}` |
| `rust` | `rustc --edition 2021 -C opt-level=3 -o {out} {src}` | `rustc --edition 2021 -C opt-level=3 -C target-cpu=native -o {out} {src}` | `{out}` |
| `go` | `go build -o {out} {src}` | `GOAMD64=v3 go build -o {out} {src}` | `{out}` |
| `python` | n/a | n/a | `python3 {src}` |
| `typescript` | n/a | n/a | `deno run --allow-read {src}` |
| `swift` | `swiftc -O -o {out} {src}` | n/a | `{out}` |
| `koka` | `koka -O2 -o {out} {src}` | n/a | `{out}` |
| `pypy` | n/a | n/a | `pypy3 {src}` |

The headline charts use the **portable** build of every language. The native
build is a separate dimension, charted separately, and only for the languages
that have one; a language without one shows `n/a` there rather than its portable
number under a native label. Go's "native" build targets the x86-64-v3 feature
level rather than the exact host CPU, and is labelled `x86-64-v3`; off x86-64 it
is the portable build and is labelled as such.

Each line is the optimised release build the language's own documentation
recommends. Rust's line matches the optimisation level of its `--release`
profile. Deno 2 does not type-check on `deno run`, so no type-checking time is
in a TypeScript run.

What each language checks at run time differs, and is part of what is measured,
not tuned away: bounds checks are kept by Miri, Rust, Go, Swift, Python and Deno
and absent in C and C++. Integer overflow traps in Miri and Swift only; Rust's
release build and Go wrap, and Python and Deno have no fixed-width integers. No
language's checks are switched off (Swift is built with `-O`, never
`-Ounchecked`).

Miri's `--opt-level 3` selects its code generator's highest optimisation
setting. A Miri number is publishable only from a compiler in which that setting
demonstrably reaches code generation: before a publication run the runner
compiles a fixed probe program at `--opt-level 0` and `--opt-level 3`, and
refuses to run if the two produce identical machine code. The runner records the
code generator setting each Miri build used.

## 5. Metrics

A run invokes the program once. A **cold** run passes only the input path: the
program reads the input, runs the kernel once, prints the checksum and exits. A
**hot** run also passes `hot`: the program reads the input, runs the kernel
`runs.hot_warmup_iterations` times untimed, then `runs.hot_timed_iterations`
times, reading the language's monotonic clock around each, and prints one
`hot_ns=<integer>` line per timed iteration to stderr. The run's `hot_ns` is the
median of those lines; every line is kept in the record.

The kernel is pure with respect to its input: a kernel that would modify its
input works on a copy made before the clock starts. Every iteration, timed or
not, computes the checksum, and the program exits with a non-zero status if any
iteration's checksum differs from the first; that comparison is what keeps a
compiler from discarding the untimed iterations. The checksum printed in hot
mode must equal the cold one.

Hot runs are made at sizes S and M, with the same run counts as cold runs. At
size L the cold run itself lasts long enough that start-up and warm-up are a
small part of it.

| Metric | Measured by | Meaning |
|---|---|---|
| `process_wall_ns` | runner | Wall-clock time from `exec` to exit of a cold run. Includes start-up, input reading, interpreter or JIT warm-up and teardown. **The headline time metric.** |
| `user_ns`, `sys_ns` | runner | CPU time in user and kernel mode for the cold run, from the child's resource usage, published beside the wall time so that work done on service threads is visible. |
| `max_rss_bytes` | runner | Peak resident set size of the cold run, from the child's resource usage, normalised to bytes (Linux reports KiB, macOS bytes). Always published. |
| `hot_ns` | program | The median timed kernel iteration of a hot run (above). The post-warm-up time, with start-up excluded. |
| `floor_rss_bytes` | runner | `max_rss_bytes` of a hello-world program built and run by the same toolchain and line. What the runtime costs before the program does anything. |
| `net_rss_bytes` | derived | `max_rss_bytes − floor_rss_bytes`: an estimate of the memory the workload itself holds. **The headline memory metric**, always shown beside `max_rss_bytes`. Peak resident sizes do not subtract exactly, so a result at or below zero is published as `≤ floor`, never as a number. |
| `binary_bytes` | runner | Size on disk of `{out}` as the build line leaves it, unstripped. `n/a` for languages that run from source. |
| `startup_ns` | runner | `process_wall_ns` of the hello-world program. |

stdout carries the checksum and nothing else. stderr carries the `hot_ns` lines
in hot mode and nothing the runner reads otherwise. Both time metrics are
published: the headline one includes start-up, so start-up is never hidden, and
the hot one excludes it, so a JIT language is not judged on its warm-up alone.

The runner measures from outside the process (`wait4` resource usage and its
own monotonic clock), never from anything the program reports about itself
except `hot_ns`, which only the program can know.

## 6. Statistics

- **Warm-up.** Each cell — one workload, language, size, build variant and mode
  — starts with `runs.process_warmup_runs` runs that are discarded.
- **Measured runs.** Then `runs.measured.s`, `runs.measured.m` or
  `runs.measured.l` runs, by size. The counts are odd so the median is an
  observed run. Every measured run is kept: no outlier is removed, ever.
- **Reported.** Median, minimum, the `statistics.p_high` quantile and the
  coefficient of variation (CV: sample standard deviation over mean) of every
  metric. Quantiles use the nearest-rank method. Charts show the median.
- **Noisy cells.** A cell whose CV of `process_wall_ns`, `hot_ns` or
  `max_rss_bytes` exceeds `statistics.noisy_cv` is re-run from its warm-up, up
  to `statistics.noisy_max_reruns` times. Every attempt's runs are kept and
  published; the last attempt is the one charted. A cell still noisy after the
  last re-run is published with a `noisy` flag, never dropped.
- **Ratio to C.** Each cell's median is divided by the median of the C cell of
  the same workload, size, build variant and mode from the same run. Comparisons
  across runs — across days, machines or CI hosts — use these ratios, never
  absolute nanoseconds. The CV of a ratio is taken as
  `√(CV_cell² + CV_C²)`.
- **Cross-version verdict.** Comparing Miri version A with version B on a cell
  is allowed only between runs on the same machine profile. With `r` the ratio
  of B's median ratio-to-C over A's, and `t` the larger of
  `verdict.min_threshold` and `verdict.cv_multiplier` times the larger of the
  two ratio CVs, the verdict is `faster` when `r < 1 − t`, `slower` when
  `r > 1 + t` and `same` otherwise. A change smaller than the noise is `same`;
  it is never reported as a win.
- **Time cap.** A run — measured or warm-up — longer than
  `timeout.c_median_factor` times the C median for that cell, or
  `timeout.floor_s` seconds if that is longer, is killed and retried
  `timeout.retries` times. A run killed on its last retry marks the cell
  `timeout`: its remaining runs are skipped, and the chart shows it as a
  timeout at its cap rather than leaving it out. If the C cell itself failed or
  timed out, the run is not a publication run. A slow language is a result, not
  a reason to stall the run.

## 7. Sizes

Every workload runs at three sizes, S, M and L, defined by how long the
**portable C build** takes on the publication profile: `process_wall_ns` near
`sizes.target_c_wall_ms.s`, `.m` and `.l`. Each size is long enough for every
language's timer to resolve it.

Calibration is its own run, made once per workload on the `publication` profile
with the portable C build only, before any other language of that workload has
been timed. It picks the input parameter that brings the C median within
`sizes.calibration_tolerance` of each target and writes it, with the
calibration runs, into the workload's manifest; the manifest is committed and
its hash is in every record. From then on the parameters are frozen: changing
one is a charter change (section 10). All three sizes are always run and always
published together.

## 8. Machine profiles

| Profile | Machine | May appear in a published chart |
|---|---|---|
| `publication` | A dedicated Linux x86-64 machine: CPU frequency governor fixed at `performance`, turbo and frequency boost off, the benchmark on one core isolated from the scheduler with its hyper-thread sibling idle, nothing else running, toolchains from the pinned container image. | **Yes — the only one.** |
| `dev` | A developer's workstation. | No. For diagnosis and for checking a change before a publication run. |
| `ci-smoke` | A shared CI runner, size S only. | No. A regression signal on ratios to C, read with the verdict rule. |

On `publication` the runner checks the governor, the boost state, the core's
isolation and the sibling's idleness before each cell, records what it found,
and refuses the `publication` label to a run in which any check failed. The
whole process tree is pinned to the one isolated core, a runtime's service
threads included, so no language gets free cores the others do not; `user_ns`
and `sys_ns` show what those threads cost.

Input files live on a memory-backed file system, so the page cache does not
depend on which run came first. Address-space layout randomisation stays at the
operating system's default.

The child's environment is empty except for `PATH`, `HOME`, `PYTHONHASHSEED=0`
(fixed for deterministic output, not speed) and a pinned `DENO_DIR`, plus any
variable a build line names. No `MIRI_*`, `GO*`, `MALLOC_*`, `PYTHON*` or
`DENO_*` variable is inherited. Miri's leak check and heap guard run in the
untimed identity gate, never in a timed run.

## 9. The publication gate

Every number in an article, on the website's benchmark page or in a chart is
generated from a committed `summary.json` produced by the statistics stage from
committed raw records under one charter version and one `publication` run. A
hand-typed number anywhere in a published artifact is a defect, and a gate
checks for it. A summary that mixes machine profiles or charter versions is
refused.

A cell where Miri's `process_wall_ns` ratio to C is at least
`diagnosis.time_vs_c`, or its `net_rss_bytes` is at least
`diagnosis.net_rss_vs_rust` times Rust's, is a loss worth explaining. Time is
measured against C, the fastest reference; memory against Rust, the reference
that frees deterministically with standard containers (section 2). Such a cell
is published as it is, diagnosed to a mechanism, and the mechanism is filed as
compiler work before the article goes out.

## 10. Changing the charter

A change to this document or to `charter.toml` — a threshold, a build line, a
size parameter, a language, a workload's removal — gets a new
`charter_version`, a dated entry in the changelog below saying what changed and
why, and applies from the next run onward. A new version re-runs every cell; a
published summary shows every workload under one version, and the summary
published under the previous version stays published. Results under different
charter versions are never charted together, and a result is never re-labelled
under a version it was not run under. A workload, once run, is removed only by a
charter change that names it and its last result.

## 11. What a hostile reader will say, and the answer

- **"They tuned Miri and not the others."** The build lines are fixed in section
  4 for every language, each the optimised release build its own documentation
  recommends; Miri gets no flag the others are denied, and its optimisation
  setting is proven to take effect before a run counts. Miri ports follow the
  same checklist, and rewriting a port around a Miri weakness is forbidden.
- **"They hid start-up."** The headline time is whole-process wall clock,
  start-up included; start-up and the hello-world floor are published
  separately as well.
- **"They cherry-picked sizes."** Sizes are defined by C's runtime in a
  calibration run made before any other language was timed, frozen in a
  committed manifest whose hash every record carries, and all three are always
  published.
- **"They cherry-picked workloads."** Every workload is declared before its
  first run, and every workload that has ever run is published. Dropping one is
  a dated charter change naming its last result.
- **"They cherry-picked runs."** Every measured run, and every attempt of a
  re-run noisy cell, is kept and published; no outliers are removed; medians are
  published with the minimum, the upper quantile and the CV beside them.
- **"The ports don't do the same work."** The identity gate requires
  byte-identical output from every language before anything is timed, and every
  port carries a review by someone other than its author, bound to its exact
  text by a hash.
- **"Their C is bad C."** The ports are public, idiomatic and reviewed; a better
  port that still meets the checklist is accepted and the whole workload is
  re-run with it.
- **"Python and TypeScript were judged on JIT warm-up."** `hot_ns` is the median
  of several timed iterations after several warm-up iterations, PyPy is offered
  as an optional arm, and the time cap is set wide enough that a slow
  interpreter is measured rather than cut off. `hot_ns` is a post-warm-up
  measurement, not a proof that every JIT reached its top tier.
- **"Miri is fast because it leaks."** Miri ports pass the leak check and the
  heap guard in the identity gate, and `max_rss_bytes`, which counts everything
  the process holds, is published beside the net figure.
- **"The machine was noisy."** Published numbers come only from the
  `publication` profile, whose state the runner checks before every cell; CV is
  published for every cell, and a difference inside the noise is reported as
  `same`.
- **"Garbage-collected languages got spare cores."** Every process tree is
  pinned to one core, and CPU time is published beside wall time.
- **"They only show where Miri wins."** Every cell is published. Losses past the
  diagnosis thresholds are explained, not removed.
- **"The rules were changed after the results."** This file was committed before
  the first measured run; the changelog below is its complete history; every
  result carries the hash of the exact charter, manifest, runner and image it
  ran under, and a new charter version re-runs everything.

## Parameters

The values in `charter.toml`, as this document defines them. The test that
checks the two files agree reads this table.

| Key | Value | Meaning |
|---|---|---|
| `charter_version` | `1` | The version every record carries (section 1). |
| `runs.process_warmup_runs` | `2` | Discarded runs at the start of each cell. |
| `runs.hot_warmup_iterations` | `5` | Untimed kernel iterations at the start of a hot run. |
| `runs.hot_timed_iterations` | `5` | Timed kernel iterations in a hot run; `hot_ns` is their median. |
| `runs.measured.s` | `21` | Measured runs per cell at size S. |
| `runs.measured.m` | `11` | Measured runs per cell at size M. |
| `runs.measured.l` | `7` | Measured runs per cell at size L. |
| `sizes.calibration_tolerance` | `0.3` | Largest relative distance of the C median from a size's target after calibration. |
| `sizes.target_c_wall_ms.s` | `50` | C reference wall time for size S, in milliseconds. |
| `sizes.target_c_wall_ms.m` | `500` | C reference wall time for size M, in milliseconds. |
| `sizes.target_c_wall_ms.l` | `3000` | C reference wall time for size L, in milliseconds. |
| `statistics.noisy_cv` | `0.05` | CV above which a cell is re-run. |
| `statistics.noisy_max_reruns` | `2` | Re-runs before a noisy cell is published flagged. |
| `statistics.p_high` | `0.9` | The upper quantile reported beside the median. |
| `verdict.min_threshold` | `0.05` | Smallest relative change a cross-version verdict calls faster or slower. |
| `verdict.cv_multiplier` | `3` | How many ratio CVs a change must exceed to be called faster or slower. |
| `timeout.c_median_factor` | `300` | A run longer than this many C medians is killed. |
| `timeout.floor_s` | `10` | The time cap never falls below this many seconds. |
| `timeout.retries` | `1` | Retries of a killed run before the cell is marked `timeout`. |
| `diagnosis.time_vs_c` | `1.5` | Ratio to C at which a Miri time cell is diagnosed. |
| `diagnosis.net_rss_vs_rust` | `1.5` | Ratio to Rust at which a Miri memory cell is diagnosed. |

## Changelog

- **Version 1, 2026-10-15** — first version, reviewed for the objections in
  section 11 and committed before any measured run.
