# Agent-loop evaluation harness

The agent contract — stable diagnostic codes, a JSON envelope, `explain`, `fix`,
`view`, `patch` — is only worth its cost if it measurably shortens tool-driven
work. This directory is the measuring device, and `evals/results/baseline.md` is
what it currently measures.

## What a task is

Each directory here is one recorded transcript: the ordered sequence of
invocations a tool would issue to finish one job.

```
evals/<id>/steps.toml   the transcript
evals/<id>/seed/        the files the job starts from
```

What is replayed is the *agent's decisions*. The compiler is the real binary and
its output is never recorded or mocked, so a change in the compiler moves the
numbers. Each task runs in a fresh temporary directory seeded from `seed/`; the
fixtures themselves are never written to.

A task with no `seed/` directory starts from an empty one. That is deliberate —
git cannot carry an empty directory, so its absence is how "starts from nothing"
is represented.

## The metrics

| Column | What it counts |
|---|---|
| `success` | Every step's assertions held. |
| `invocations` | Compiler invocations. Writing a file is the agent's own work and is not one. |
| `bytes_read` | Normalized stdout and stderr the loop had to ingest. |
| `bytes_written` | `.mi` source the loop caused to be written, by the agent or by the compiler. |

All four are gated. Every one is observed by the harness from the outside; none
is read from a field the compiler populates about itself. A measuring device
must not ask its subject for its own score — were the number self-reported, a
regression that stopped reporting it would read as an improvement.

Byte counts are taken over *normalized* output. Two things in the envelope vary
between identical runs — `durationMs`, and absolute paths carrying the temporary
directory's name — and counting them raw would make the baseline unreproducible.

**Wall-clock is measured and printed, never committed.** It records the load on
whichever machine ran the suite rather than the cost of the loop, and putting it
in a committed file would rewrite that file on every run.

## Running it

```sh
make evals-replay   # replay and compare against the committed baseline
make evals-bless    # re-record the baseline
```

A run that no longer reproduces the baseline fails, and the failure names the
task and the columns that moved:

```
task b: invocations: 6 -> 7, bytesRead: 1934 -> 2051
```

**A run that gets *cheaper* fails too.** The table records what the loop costs
today, not a ceiling it must stay under: a change that makes the loop cheaper
should show up as a deliberate edit to that record, in the diff that earned it.
Re-record with `make evals-bless` and commit the updated table.

## Adding a task

Add a directory with a `steps.toml` and a `seed/`, then add its id and one-line
description to `TASKS` in `tests/evals/mod.rs`. The list is explicit rather than
discovered by reading this directory, so a fixture that goes missing fails the
run instead of silently shrinking the corpus.

Two guards constrain what a transcript may look like, and both exist because a
measuring device that passes while measuring nothing is worse than none:

- Every task must assert something about what the compiler *said* — a diagnostic
  code, or a string in its output. Exit codes alone measure the loop's length
  without measuring whether it worked.
- Every task must either change a file or recover from a genuine failure (a step
  recorded as failing, followed by one recorded as succeeding).

An unknown key in a `steps.toml` is rejected at load. A mistyped assertion name
would otherwise leave a fixture that asserts nothing and still reports success.

## Step types

| `type` | Runs |
|---|---|
| `WriteFile` | Not a compiler invocation: the agent authoring content itself. |
| `Check` | `miri check <file> --format json` |
| `Explain` | `miri explain <code>` |
| `FixPlan` / `FixApply` | `miri fix <file> --plan` / `--apply --yes` |
| `ViewFn` / `ViewOutline` | `miri view <file> --fn <name>` / `--outline` |
| `Patch` | `miri patch <file> --replace-in-fn <fn> --old <t> --new <t>` |
| `ReplaceFn` | `miri patch <file> --replace-fn <fn> --body-file …` |
| `Run` / `Build` / `TestDir` | `miri run` / `build` / `test --dir` |

Assertions available on any step: `must_succeed` (default true, enforced in both
directions), `assert_diagnostic_code`, `assert_output_contains`,
`assert_file_changed`.

## A caveat about where the baseline was recorded

The committed numbers were recorded on macOS; CI runs ubuntu-24.04. Nothing the
harness measures embeds the repository's path, and the working directory is
normalized — including the `/private` form macOS resolves temporary directories
through — so the counts should carry across. They are not yet *proven* to. If
the gate fails on a first CI run with small byte deltas and no other change, the
fix is to widen the normalizer, never to widen the gate.

## A known gap the numbers record

`miri patch` cannot insert a sibling declaration — it refuses with
`MER_BLD_010`. Task `c` therefore authors its new function with a direct write
rather than through the edit surface. That write is the honest current cost of
that loop, and `bytes_written` records it rather than hiding it. When a
declaration-inserting operation lands, task `c` should move onto it and the
baseline should get cheaper.
