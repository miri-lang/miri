# The live-model benchmark

This directory is the instrument. It measures what it costs a language model
that was never trained on Miri to reach a *correct* program, next to the same
model working in languages it knows cold.

It is not the replay corpus one level up. `evals/` replays a recorded
transcript against the real compiler and gates the numbers in continuous
integration; it is deterministic, cheap, and it measures the loop's shape rather
than a model's behaviour. This directory launches live models, costs money, and
is run by hand. Neither substitutes for the other: a change that moves the
replay numbers has not been shown to move an agent's cost, and a round here is
too expensive and too noisy to gate a commit.

## What a run is

A **cell** is one job, one arm and one model. A **round** is one complete pass
over every cell, three runs each. One run is a single non-interactive launch of
an agent harness with no human turn in it, in a scratch directory outside this
repository, followed by the hidden tests.

```
PROMPT.md                  the text a round is launched from, unedited
jobs/<job>/BRIEF.md        the job, with three substitutions
jobs/<job>/job.toml        its caps and title
jobs/<job>/cases/*.in,.out the hidden tests
jobs/<job>/seeds/<lang>/   what the subject starts from, per language
jobs/<job>/reference/      the program the expectations come from
arms.toml                  the arms
onboarding/                what an onboarded arm is taught from first
models.toml                the three model columns
bench.py                   runs one cell and writes its records
report.py                  folds a round's records into summary.json
runs/<round>/...           the records and transcripts
runs/<round>.probe/...     the opinion probe's ratings, never folded into a claim
```

## The arms

| Arm | What the subject has |
|---|---|
| `miri-pack` | The release binary with the skill pack installed in the workspace. |
| `miri-bare` | The release binary and `miri --help`. No prose file of any kind. |
| `miri-skill` | The release binary, taught from the skill pack before the brief. |
| `miri-grammar` | The release binary, taught from the formal grammar before the brief. |
| `miri-both` | The release binary, taught from the pack and the grammar together. |
| `python` | The language and its toolchain. |
| `rust` | The language and its toolchain. |

The baselines get no skill file. Their guidance is in the weights, and that
asymmetry **is** the experiment: `miri-pack` against `python` asks whether a
pack can stand in for pretraining, and `miri-pack` against `miri-bare` asks
whether the pack is what is doing the standing.

### Onboarded arms

Python and Rust arrive taught: what a model knows of them is in its weights, an
advantage no page can match. The onboarded arms ask a different question: once
an agent has been taught Miri, what does a program cost? Each is two launches of
one harness session. The first hands the subject `onboarding/ONBOARDING.md` —
the same text for every job, naming nothing job-specific — with only the arm's
material in the workspace (`onboarding/materials.toml` says which, and copies
it in from the operator's side). The subject reads it and gets a small warm-up
program of its own to run. Only then are the job's seed and brief laid in, and
the session is resumed on the measured prompt.

Everything a record measures is the second launch's. The onboarding phase's
own cost, turns and transcript are kept beside it under `onboarding` and in
`<run>.onboarding.jsonl`, never added in; but the onboarding context stays in
the conversation, so every measured turn still pays to re-read it as cached
input, and the measured numbers carry that cost. A run is clean only if both
launches were. The baselines and the two cold Miri arms stay single-launch.

## The jobs

| Job | Starts from | What it exercises |
|---|---|---|
| `01-word-frequency` | nothing | Authoring a whole small program from a contract. |
| `02-ledger-repair` | a committed program carrying six planted faults | Repair: finding faults nothing points at. |
| `03-tracker-extension` | a committed working program | Extension: changing code the subject did not write. |
| `04-data-edges` | nothing | The edges a hurried program skips — empty input, text outside the Latin alphabet, an absent key, an overflowing total. |
| `05-gpu-heat` | nothing | A GPU kernel. |
| `06-gpu-life` | nothing | Integer kernels with double buffering and an edge rule. |
| `07-gpu-escape-time` | nothing | A float kernel, scored with a tolerance on a histogram of escape times. |
| `08-gpu-histogram` | nothing | A reduction: values generated and counted on the device, atomics or several passes. |
| `09-gpu-particles` | nothing | A simulation of two kernels per step, scored with a tolerance on summary statistics. |

The GPU jobs from `06` on are compute-only versions of the demos the website
runs (`examples/gpu/web/`), chosen by domain before any pilot ran. Each ships a
Miri solution and a Python one on `wgpu` beside its reference, both run on a GPU
against the hidden cases when the job was written, which is what shows the job
can be won; a later round may tune a job's caps or tolerance, never drop it
because one arm loses it.

Every brief is language-neutral, and the three substitutions — the language
name, the toolchain line and the entry point — are the only text that differs
between arms. The planted faults of `02-ledger-repair` are the same six
*semantic* faults at the same points in all three ports; `FAULTS.md` there is the
map, and a structural test in this repository fails when a port stops carrying
one. Faults are never syntax errors: a fault a compiler points at measures the
compiler, and it would measure a different one in every language.

### Why the input arrives as a file path

Each job's program reads its input from the path in its first command-line
argument, not from standard input. Miri has no way to read standard input
today — there is no runtime intrinsic and no standard-library call for it — so a
contract written on standard input would be a contract one arm cannot satisfy
at all. Naming a file is equally ordinary in every arm and advantages none
of them.

## The rules a run is subject to

- **Unattended.** One prompt, permissions auto-approved, no human turn, no
  follow-up. What the agent leaves behind when it stops is what is scored.
- **Barred from this repository.** The workspace is outside it, and nothing the
  subject is handed points back at it. `bench.py` stages a copy of the operator's
  `miri`, its standard library and its runtime libraries under the scratch
  directory and puts that copy first on `PATH`; the subject's environment keeps
  no variable that names this repository and no `PATH` entry inside it. A run
  whose tool calls name the repository anyway is recorded as not isolated, and
  `report.py` refuses its round.
- **Hidden tests stay hidden.** They are never in the workspace and are run
  only after the agent has stopped, so a green run passed the contract rather
  than the cases.
- **A measured arm is never asked for a per-tool verdict.** Asking a subject
  what it thought of a tool buys invocations a real job would not spend, and it
  inflated the tooled column of the 2026-09-09 numbers. Opinions are collected
  afterwards, in a separate probe that is not measured: `bench.py --probe`
  re-runs one cell per arm, asks the subject to rate every surface it used in
  `RATINGS.json`, and writes the ratings to `runs/<round>.probe/` beside the
  round. Nothing a subject says about a tool reaches a claim: `report.py` reads
  that directory for one verdict only, on the opinion condition of the exit
  criterion, and it refuses — rather than skips — a probe record found inside a
  round or a measured record found beside one.
- **Invocations lost to compiler defects are counted separately** from the
  clean loop. A defect is a fact about the compiler, not about the surface
  wrapped around it, and folding the two together is what made two earlier
  rounds disagree with each other. The count is a diagnostic: every verdict is
  computed from gross numbers, because a user of the language pays for its
  defects as well.
- **Caps are identical across arms** and live in each `job.toml`.

## How a hidden test is judged

A job's output is compared exactly unless its `job.toml` carries a `[compare]`
table with `kind = "numeric"`. Then a field that reads as a number on both sides
matches within the larger of `absolute` and `relative` times the expected value,
and every other field still matches exactly. A line whose first field is a word
can be held to its own tolerance under `[compare.labels.<word>]`. A GPU computes
in single precision while the references compute in double, so a float job
states its tolerance next to the measurement that sized it. A failed case
records its first differing value under `failureDetail`.

A job with `gpu = true` asks for a kernel, and a program that prints the right
numbers from a loop on the processor answered a different job. Each arm says in
`arms.toml`, under `kernel_evidence`, which files a kernel would be written in
and which patterns show one is written and dispatched: a device `forall` for
Miri, a WGSL `@compute` shader and a `dispatch_workgroups` call for the
baselines. A run missing any of them is recorded `notOnGpu`; it is never green,
and a run that declared itself finished without one is a silent wrong answer.

The TypeScript arm was dropped on 2026-10-11, when the benchmark turned toward
GPU compute: Python and Rust remain the baselines. Rounds recorded before then
keep their TypeScript cells.

## What is measured

All of it from outside the run: tokens in and out, turns, tool invocations,
toolchain invocations, wall clock, the outcome, the hidden-test pass rate at the
moment the agent stopped, and whether the run was a **silent wrong answer** — a
run the agent declared finished that fails a hidden test. The primary numbers
are tokens-to-green and silent wrong answers. Toolchain invocations are
secondary and never summed across languages: `cargo build` and `miri check` are
not the same unit of work.

Tokens are the ones the harness settles in its closing `result` event. Input
counts every token the model processed, cache reads and cache writes included,
and the cached parts are also recorded apart; a run whose stream never settled,
because it hit the wall-clock cap, records its tokens as unobserved rather than
as zero. A turn is one model response, however many events the harness split
it into.

Nothing is read from a field the subject populates about itself.

## Running one cell

```sh
# What the subject will see, without launching anything.
python3 evals/field/bench.py --job 02-ledger-repair --arm rust --dry-run

# One run of one cell.
python3 evals/field/bench.py --job 02-ledger-repair --arm rust --model claude-sonnet

# The three runs a round wants, into a named round directory.
python3 evals/field/bench.py --job 02-ledger-repair --arm rust \
    --model claude-sonnet --runs 3 --round r1

# Score a workspace again without re-running the agent.
python3 evals/field/bench.py --job 02-ledger-repair --arm rust \
    --score-only "$TMPDIR/miri-field/r1/02-ledger-repair/rust/claude-sonnet/1"

# Score a recorded run again after a defect in the scorer, rewriting its
# record; the score it replaces is kept under `rescored.previous`.
python3 evals/field/bench.py --job 05-gpu-heat --arm python \
    --rescore evals/field/runs/r1/05-gpu-heat/python/claude-sonnet/1.json

# Fold the round and judge the claims.
python3 evals/field/report.py --round r1

# After the round, one unmeasured opinion probe per arm.
python3 evals/field/bench.py --job 02-ledger-repair --arm miri-pack \
    --model claude-sonnet --round r1 --probe
```

A probe prints each rating it read. A probe whose subject left no readable
`RATINGS.json` — missing, not JSON, or a score outside 1 to 5 — keeps its record
and transcript with the problem named in `ratingsProblem`, and exits non-zero.

`bench.py` needs the `claude` harness on the path (or a wrapper around it,
named with `--harness-command` and recorded in every record), the arm's
toolchain, and a `miri` on the path for the Miri arms. A model column whose
`id` is empty in `models.toml` refuses to run until `--model-id` pins the exact
model that ran: a round whose model moved under it is not a round.

A subject runs apart from the operator's own Claude Code configuration. The
harness is launched with only the user-independent setting sources, no MCP
servers, and `harness-settings.json`, which pins the subscription login and
carries no hook; passed last, it replaces any settings a wrapper passed first.
The subject's environment loses every `CLAUDE*`, `ANTHROPIC_*` and `AWS_*`
variable and `MAX_THINKING_TOKENS`, so nothing routes it to another backend or
account and the model runs at the harness's defaults. The scratch
directory defaults to the system temporary directory, and a scratch directory
with a `CLAUDE.md` in any ancestor is refused, because the harness reads those
on its way up. Each record carries what the harness reported loading under
`isolation`; `report.py` refuses a round holding a run that was not clean — a
hook that ran, an MCP server, a plugin the harness does not ship — since such
a run measured the operator's setup as well as the language.

## The claims

`PROMPT.md` holds the text a round is launched from. A round is started by
handing an agent that block unedited, never by typing a fresh instruction: the
rounds that predate this instrument each drifted into a different experiment,
and a prompt under version control cannot drift without the drift showing in a
diff.

`CLAIMS.md` holds the pre-registered claims, and `report.py` computes each
verdict from the records — no verdict is typed by hand. It computes every
condition of the exit criterion too: that no Miri run answered wrongly in
silence, that the packed loop costs no more invocations than the bare one, and
that no surface the published page recommends came back rated 2 out of 5 or
lower. That last one is joined against `skills/surfaces.toml`, which lists the
surfaces the page recommends and which the packs themselves are gated against —
a free-text surface name would match nothing, and a verdict that matched nothing
would read as held by having asked no one. `report.py` also labels every
comparison a lead claim loses as `parity` or `behind`, so a failing round still
shows its distance from the claim. `baseline.md` holds the
2026-09-09 numbers that predate this instrument, with the reasons they are not
comparable to anything produced here. `LOG.md` carries one entry per round.
