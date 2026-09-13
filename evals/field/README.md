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
jobs/<job>/BRIEF.md        the job, with three substitutions
jobs/<job>/job.toml        its caps and title
jobs/<job>/cases/*.in,.out the hidden tests
jobs/<job>/seeds/<lang>/   what the subject starts from, per language
jobs/<job>/reference/      the program the expectations come from
arms.toml                  the five arms
models.toml                the three model columns
bench.py                   runs one cell and writes its records
report.py                  folds a round's records into summary.json
runs/<round>/...           the records and transcripts
```

## The arms

| Arm | What the subject has |
|---|---|
| `miri-pack` | The release binary with the skill pack installed in the workspace. |
| `miri-bare` | The release binary and `miri --help`. No prose file of any kind. |
| `python` | The language and its toolchain. |
| `rust` | The language and its toolchain. |
| `typescript` | The language and its toolchain. |

The baselines get no skill file. Their guidance is in the weights, and that
asymmetry **is** the experiment: `miri-pack` against `python` asks whether a
pack can stand in for pretraining, and `miri-pack` against `miri-bare` asks
whether the pack is what is doing the standing.

## The jobs

| Job | Starts from | What it exercises |
|---|---|---|
| `01-word-frequency` | nothing | Authoring a whole small program from a contract. |
| `02-ledger-repair` | a committed program carrying six planted faults | Repair: finding faults nothing points at. |
| `03-tracker-extension` | a committed working program | Extension: changing code the subject did not write. |
| `04-data-edges` | nothing | The edges a hurried program skips — empty input, text outside the Latin alphabet, an absent key, an overflowing total. |
| `05-gpu-heat` | nothing | A GPU kernel. |

Every brief is language-neutral, and the three substitutions — the language
name, the toolchain line and the entry point — are the only text that differs
between arms. The planted faults of `02-ledger-repair` are the same six
*semantic* faults at the same points in all four ports; `FAULTS.md` there is the
map, and a structural test in this repository fails when a port stops carrying
one. Faults are never syntax errors: a fault a compiler points at measures the
compiler, and it would measure a different one in every language.

### Why the input arrives as a file path

Each job's program reads its input from the path in its first command-line
argument, not from standard input. Miri has no way to read standard input
today — there is no runtime intrinsic and no standard-library call for it — so a
contract written on standard input would be a contract one arm cannot satisfy
at all. Naming a file is equally ordinary in all five arms and advantages none
of them.

## The rules a run is subject to

- **Unattended.** One prompt, permissions auto-approved, no human turn, no
  follow-up. What the agent leaves behind when it stops is what is scored.
- **Barred from this repository.** The workspace is outside it, and nothing in
  the workspace points back at it. `MIRI_STDLIB_PATH` is set so the compiler
  finds its own standard library without the subject reading this tree.
- **Hidden tests stay hidden.** They are never in the workspace and are run
  only after the agent has stopped, so a green run passed the contract rather
  than the cases.
- **A measured arm is never asked for a per-tool verdict.** Asking a subject
  what it thought of a tool buys invocations a real job would not spend, and it
  inflated the tooled column of the 2026-09-09 numbers. Opinions are collected
  afterwards, in a separate probe that is not measured.
- **Invocations lost to compiler defects are counted separately** from the
  clean loop. A defect is a fact about the compiler, not about the surface
  wrapped around it, and folding the two together is what made two earlier
  rounds disagree with each other.
- **Caps are identical across arms** and live in each `job.toml`.

## What is measured

All of it from outside the run: tokens in and out, turns, tool invocations,
toolchain invocations, wall clock, the outcome, the hidden-test pass rate at the
moment the agent stopped, and whether the run was a **silent wrong answer** — a
run the agent declared finished that fails a hidden test. The primary numbers
are tokens-to-green and silent wrong answers. Toolchain invocations are
secondary and never summed across languages: `cargo build` and `miri check` are
not the same unit of work.

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
    --score-only ~/.cache/miri-field/r1/02-ledger-repair/rust/claude-sonnet/1

# Fold the round and judge the claims.
python3 evals/field/report.py --round r1
```

`bench.py` needs the harness on the path (`claude`, `gemini`), the arm's
toolchain, and a `miri` on the path for the Miri arms. A model column whose
`id` is empty in `models.toml` refuses to run until `--model-id` pins the exact
model that ran: a round whose model moved under it is not a round.

## The claims

`CLAIMS.md` holds the four pre-registered claims, and `report.py` computes each
verdict from the records — no verdict is typed by hand. `baseline.md` holds the
2026-09-09 numbers that predate this instrument, with the reasons they are not
comparable to anything produced here. `LOG.md` carries one entry per round.
