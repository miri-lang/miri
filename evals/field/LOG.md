# Round log

One entry per round, appended by whoever ran it, newest last. A round in which
any claim fails is logged here in full rather than discarded; the article links
every round it did not publish, with the verdict recorded here.

An entry names: the round, the date, the compiler commit, the model ids, what
moved since the previous round, the verdict per claim as `report.py` computed
it, the ratings its opinion probe collected under `runs/<round>.probe/`, and —
for a re-run under the prompt that drives this benchmark — the verdict on each
condition of the exit criterion, as `report.py` computed that too.

The numbers that predate this instrument are in `baseline.md`, and they are not
a round.


## r1 — 2026-10-11

**Compiler and instrument.** The Miri arms ran at `e2ed19af` (release build,
`miri 0.7.0-beta.5`, macOS aarch64); the baseline arms ran between `c751bbd8`
and `fa88d752`, commits that changed only the instrument, never what a baseline
subject is handed. Models `claude-haiku-5-5`, `claude-sonnet-5-5` and
`claude-opus-5-5` through Claude Code 2.1.296 on a subscription login, every
record isolated (`apiKeySource` none, no hook, no MCP server, no repository
reference). 225 measured runs and 5 probes.

**What moved since the previous measurements.** This is the first round on this
instrument, so it has no predecessor to delta against and establishes the table
below. Against the hand-run field tests in `baseline.md` (not comparable, and not
quoted here): `fix --plan` and the agent protocol's plan method no longer exist,
and `miri --help` now carries the recommended loop, so the bare arm reads it too.

**Getting a valid round took four instrument fixes, made before any record was
kept.** Turns were counted per streamed block and input tokens ignored cached
input (fixed before the first measured run). A failed hidden case kept no reason
(one Miri run read as a silent wrong answer that re-scoring could never repeat).
The python arm was scored outside the `uv` environment its subject built (three
GPU runs read as 0/5; rescored to 5/5 with the old score kept under
`rescored.previous`). And the subjects were not barred from the compiler's
repository: `PATH` and the standard-library path pointed into it, 32 of the
first 90 Miri runs read its docs, examples or sources, and one listed its own
job's hidden cases. Those 90 runs, and four from a re-run that found the
compiler printing its build tree from `miri view --stdlib-root`, were voided and
kept out of the repository; the Miri arms were then run again in full. No
baseline run touched the repository.

Each cell below reads green/runs · median tokens to green · median tool
invocations to green · median wall clock to green.

### 01-word-frequency

| Model | miri-pack | miri-bare | python | rust | typescript |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 836k · 23 · 215s | 2/3 · 1560k · 35 · 391s | 3/3 · 148k · 8 · 31s | 3/3 · 140k · 7 · 28s | 3/3 · 166k · 7 · 29s |
| claude-opus | 3/3 · 380k · 13 · 60s | 3/3 · 1021k · 27 · 300s | 3/3 · 74k · 3 · 34s | 3/3 · 54k · 2 · 21s | 3/3 · 77k · 3 · 42s |
| claude-sonnet | 3/3 · 473k · 18 · 68s | 3/3 · 685k · 22 · 238s | 3/3 · 53k · 2 · 16s | 3/3 · 52k · 2 · 14s | 3/3 · 52k · 2 · 13s |

Every arm finished except one bare Haiku run that went past the turn cap. The
packed arm cost 5 to 9 times the baselines and the bare arm 9 to 19 times, and
the transcripts say why: 12 of the 18 Miri runs wrote a first version that took 13 to 75 seconds on a large
input, because walking a string by character (`for ch in s`, `code_at(i)`) and
walking a `Map` by entry are each O(n) per step. The subjects then timed
primitives one by one and rebuilt around `split`, regex or byte-wise
`substring`. The pack beat the bare arm in every model, by the most on Opus
(380k against 1,021k).

### 02-ledger-repair

| Model | miri-pack | miri-bare | python | rust | typescript |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 504k · 16 · 69s | 3/3 · 484k · 17 · 74s | 3/3 · 127k · 5 · 30s | 3/3 · 132k · 6 · 31s | 3/3 · 116k · 5 · 34s |
| claude-opus | 3/3 · 352k · 12 · 71s | 3/3 · 302k · 12 · 78s | 3/3 · 113k · 5 · 33s | 3/3 · 96k · 4 · 34s | 3/3 · 95k · 4 · 30s |
| claude-sonnet | 3/3 · 195k · 8 · 31s | 3/3 · 235k · 10 · 39s | 3/3 · 73k · 3 · 17s | 3/3 · 74k · 3 · 22s | 3/3 · 74k · 3 · 19s |

Every run of every arm found all six planted faults. The packed arm paid 3 to 4
times the baselines' tokens for a job whose difficulty is the same in any
language, which is the cost of reading an unfamiliar API: subjects probed range
exclusivity, `compare` order and `to_int` before editing. The pack did not beat
the bare arm on Haiku (504k against 484k) or Opus (352k against 302k).

### 03-tracker-extension

| Model | miri-pack | miri-bare | python | rust | typescript |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 341k · 12 · 49s | 3/3 · 353k · 13 · 56s | 3/3 · 211k · 9 · 42s | 3/3 · 190k · 8 · 41s | 3/3 · 131k · 6 · 29s |
| claude-opus | 3/3 · 224k · 9 · 46s | 3/3 · 168k · 7 · 48s | 3/3 · 96k · 4 · 34s | 3/3 · 75k · 3 · 38s | 3/3 · 75k · 3 · 32s |
| claude-sonnet | 3/3 · 108k · 5 · 20s | 3/3 · 137k · 6 · 27s | 3/3 · 72k · 3 · 18s | 3/3 · 91k · 4 · 22s | 3/3 · 73k · 3 · 18s |

The closest job: Sonnet's packed arm is at parity with Rust on tokens and with
Python and TypeScript on wall clock. On Opus the bare arm reached green in 7
invocations against the pack's 9, the one cell where the packed loop cost more
than the bare one.

### 04-data-edges

| Model | miri-pack | miri-bare | python | rust | typescript |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 615k · 18 · 82s | 3/3 · 790k · 23 · 107s | 3/3 · 133k · 5 · 38s | 3/3 · 138k · 7 · 33s | 3/3 · 139k · 5 · 41s |
| claude-opus | 3/3 · 449k · 14 · 84s | 3/3 · 510k · 17 · 86s | 3/3 · 78k · 3 · 41s | 3/3 · 56k · 2 · 35s | 3/3 · 104k · 5 · 63s |
| claude-sonnet | 3/3 · 283k · 12 · 38s | 3/3 · 500k · 17 · 56s | 3/3 · 54k · 2 · 20s | 3/3 · 54k · 2 · 21s | 3/3 · 55k · 2 · 23s |

No arm answered wrongly in silence. One Opus pack run crashed on the empty-input
edge: matching `None` against a guarded `Some(c) if ...` arm followed by `_`
segfaults, and the subject spent 7 invocations bisecting it before rewriting the
match. The same run caught `substring` taking byte indices while `length()`
counts characters, which silently returns an empty string on non-ASCII text, and
an Opus bare run found that `int` overflow wraps silently and guarded every
addition with `checked_add`. Each of these would be a silent wrong answer in a
less careful program.

### 05-gpu-heat

| Model | miri-pack | miri-bare | python | rust | typescript |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 894k · 24 · 230s | 3/3 · 1116k · 26 · 242s | 3/3 · 211k · 8 · 70s | 3/3 · 276k · 11 · 223s | 3/3 · 304k · 11 · 59s |
| claude-opus | 3/3 · 683k · 20 · 100s | 3/3 · 1030k · 28 · 282s | 3/3 · 122k · 5 · 64s | 3/3 · 129k · 5 · 93s | 3/3 · 126k · 5 · 62s |
| claude-sonnet | 3/3 · 345k · 12 · 56s | 3/3 · 418k · 15 · 146s | 3/3 · 75k · 3 · 32s | 3/3 · 167k · 8 · 92s | 3/3 · 76k · 3 · 27s |

Every arm of every model wrote a working GPU kernel and passed every case. The
packed arm cost 4 to 6 times the cheapest baseline and the bare arm 5 to 8
times; four runs again lost time to
the O(n) character access in the input parser rather than to the kernel. The
pack beat the bare arm in every model.

### Compiler defects, from a hand pass over the Miri transcripts

Each was reproduced against the release binary at `e2ed19af`.

- **O(n) positional access** (character access, `Map`/`Set` iteration): 16 of 90
  Miri runs, about 110 invocations by a bracket from each subject's first remark
  about the slowness to its last. An estimate, not a ledger.
- **A guarded `Some` arm followed by `_` segfaults on `None`:** 1 run, 7
  invocations.
- **`substring` byte indices against `length()` characters:** 1 run, 3
  invocations.
- **Same-width integer overflow wraps silently:** 1 run, absorbed into writing
  `checked_add`.
- **Cosmetic, unnumbered:** `miri run` panics with "failed printing to stdout:
  Broken pipe" when its output is piped into `head`.

None of them produced a silent wrong answer in this round.

### Verdicts, as report.py computed them

- **C1 (cheaper than the native languages): not held.** Behind in every CPU
  comparison but one, Sonnet on 03-tracker-extension against `rust`, which is at
  parity (108k against 91k).
- **C2 (fewer wrong answers): held.** No silent wrong answer on any arm.
- **C3 (GPU in reach): not held.** Every baseline finished; `miri-pack` cost
  894k, 683k and 345k against cheapest baselines of 211k, 122k and 75k.
- **C4 (the pack substitutes for pretraining): not held.** The pack was cheaper
  than the bare arm in 12 of 15 cells; it was not on 02 for Haiku and Opus or on
  03 for Opus.
- **C5 (faster than the native languages): not held.** Behind in every CPU
  comparison except five at parity: 03 on Haiku against `python` and `rust`, on
  Opus against `rust`, and on Sonnet against `python` and `typescript`.

### Exit criterion

- **No silent wrong answer on a Miri arm: held.**
- **The packed loop costs no more invocations than the bare loop in every job:
  not held**, in one cell: 03-tracker-extension on Opus, 9 invocations to green
  against 7.
- **No recommended surface rated 2 or lower: held.** `miri check` 4,
  `miri run` 5, `miri view` 4, `miri-lang` 3. Unrated: `miri explain`,
  `miri fix`, `miri patch`, `miri test`, `miri-gpu`, `miri-testing`.
  Unrecognised: `miri --help` was rated 2 out of 5 by the bare arm's probe ("Gave
  a general overview but I did not need much from it"); it is not a recommended
  surface, so it does not decide the verdict.

**The testing loop does not end, and the skill pack stays unpublished:** the
pack-loop condition fails.

**The article cannot be written:** C1, C3, C4 and C5 fail. The standings above
are the distance; the largest single lever the transcripts show is the O(n)
positional access, which accounts for most of the gap on the two jobs that read
large inputs.
