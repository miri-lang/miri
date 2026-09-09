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

## What the corpus covers, and why that matters more than the numbers

A corpus of jobs that already work reports green through every gap it does not
contain. The tasks are therefore chosen for the *shapes* a real job has, not for
the paths known to be smooth: a struct, a class holding state, a match over an
enum with a multi-line arm, a cascade whose second error is the first one's
shadow, an API looked up rather than guessed, a fault only the run finds, a
sort, and a file with several unrelated faults rather than one.

The last is the answer to an easy misreading of this table. Task `b` repairs one
fault in six invocations; a trial repairing a file with six planted faults cost
twenty-nine, and comparing those two numbers says nothing. Task `l` is the job
of that size — four faults, three carrying a repair and one not — and its eight
invocations are what the difference actually costs: one call clears the three,
and the fourth costs four on its own.

## Tasks the loop cannot finish

A task may be pinned on a gap by giving it a `blocked_by` reason in `TASKS`. It
is replayed to the step that fails, recorded as `success: no` with what it cost
getting there, and the results table names the gap under it.

This is how the corpus carries the shape of something missing rather than
omitting it. `success` is gated like every other column, so closing the gap
makes the task finish, moves the cell to `yes` and fails the gate — the fix and
this record are updated together, or neither is. `failed_step` is gated too: a
pinned task that starts failing somewhere else is failing for a new reason, and
a corpus that could not tell those apart would hold a fixture pinned to a gap
that had already moved.

## Adding a task

Add a directory with a `steps.toml` and a `seed/`, then add a `Task` entry —
its id, a one-line description, and `blocked_by: None` unless it is pinned — to
`TASKS` in `tests/evals/mod.rs`. The list is explicit rather than discovered by
reading this directory, so a fixture that goes missing fails the run instead of
silently shrinking the corpus.

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
| `ViewType` | `miri view --type <name> --public` |
| `Run` / `Build` / `TestDir` | `miri run` / `build` / `test --dir` |

`format_json` adds `--format json` to the steps that take it — `Explain`,
`ViewFn`, `ViewOutline`, `ViewType`, `Run` and `TestDir`. The rest always ask
for JSON, because the envelope is the whole point of the step.

Assertions available on any step: `must_succeed` (default true, enforced in both
directions), `assert_diagnostic_code`, `assert_output_contains`,
`assert_file_changed`.

`assert_diagnostic_code` parses standard output alone, because an envelope lives
there by definition and a command may write a sentence for a person to standard
error beside it. `assert_output_contains` reads both, so it can assert on what
either stream said. Byte counts cover both: a loop pays for everything it has to
read.

## A caveat about where the baseline was recorded

The committed numbers were recorded on macOS; CI runs ubuntu-24.04. Nothing the
harness measures embeds the repository's path, and the working directory is
normalized — including the `/private` form macOS resolves temporary directories
through — so the counts should carry across. They are not yet *proven* to. If
the gate fails on a first CI run with small byte deltas and no other change, the
fix is to widen the normalizer, never to widen the gate.

## What task `m` shows about a diagnostic that writes its own fix

Task `m` sorts, which nothing else in the corpus does, and it costs three
invocations: a `check`, a `patch`, a `run`. The middle one is the point. The
rejection names the missing capability *and* spells the comparison the author
meant — `a.word < b.word` — so the loop reads the edit out of the diagnostic
rather than deriving it, and the fault costs no `explain` and no scoped read.
Task `l`'s fourth fault, whose diagnostic names no edit, costs four invocations
on its own. That gap is what a help line is worth.

Its `run` step is also the corpus's only value check on ordering: the three
words are built at runtime, so the printed order cannot come from the order the
string pool lays literals out in.

Task `i` grew by 45 bytes read in the same change and nothing about that task
moved: it reads `view --type String`, and `String` gained the `compare` method
that answers the ordering operators.

## What task `c` shows about the insert operation

Task `c` adds both of its declarations through `miri patch --insert-fn`, in one
call. It used to author them with a direct write, because the edit surface could
only replace text inside a declaration that already existed.

Moving onto the insert did not make the loop cheaper by these numbers, and it is
worth being precise about why. `invocations` is unchanged at four: the insert
re-checks what it wrote, so the separate `check` step went away and the patch
call took its place. `bytes_written` is unchanged at 199, because it counts the
size of the file that ends up on disk and the same file ends up there either
way. `bytes_read` rose from 316 to 702, because the patch envelope echoes each
inserted declaration back in its `edits` array, and the caller is reading text
it just sent.

What did change is not measured here: the loop no longer has to author the whole
file to add to it, and the addition is checked before it lands. These columns
count what a loop reads, writes and invokes — they do not count what the agent
had to compose to get there. That is a real limitation of the device, recorded
rather than corrected, because widening the metric to reward this change would
make it stop measuring the thing it was built for.
