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


## r2 — 2026-10-12

**Compiler and instrument.** Every arm ran on the release build of `8a4ac959`
(`miri 0.7.0-beta.5`, macOS aarch64, Metal), staged from the scorer at
`00d18787`–`e255924a`, whose later commits changed only the instrument.
Models `claude-haiku-5-5`, `claude-sonnet-5-5` and `claude-opus-5-5` through
Claude Code 2.1.296 on a subscription login. 225 measured runs, every record
isolated (`apiKeySource` none, no hook, no MCP server, no repository reference);
no opinion probe was run.

**What moved since r1.** A different experiment under re-registered claims
(`CLAIMS.md`, 2026-10-12): five GPU jobs, the four drawn from the website demos
new; Miri measured after onboarding (`miri-skill`, `miri-grammar`, `miri-both`),
the baselines cold; TypeScript dropped. Scoring now compares floats within a
job's tolerance and refuses a run whose source shows no GPU kernel. The compiler
gained, among others: the 147-line pack, `substring` in characters, overflow
traps on the host, and a fix for GPU `while a and b` loops that skipped the code
after them. Only the heat job and the Python/Rust baselines are comparable with
r1, and r1 ran a different Miri arm (`miri-pack`, cold).

**Getting a valid round took four instrument fixes during the round.** The smoke
cell found the scorer taking a `gpu fn` kernel for a processor loop (fixed before
any spend). At 04:08 the driver halted on a run that had listed the shared
scratch root, found leftover operator files naming this repository, and read its
sources; the files were moved away, the round continued in a fresh root, and the
run was repeated there. A transcript line that was not an event crashed one
cell's parser; its two lost runs were run. And the system's temporary-directory
cleaner deleted most of the staged standard library at 03:29, because staging
kept the repository's old timestamps: 25 Miri runs that started after it met a
stdlib of four modules, and all 25 were run again on a whole one. The
escape-time job carried two defects of its own — a hidden case that broke the
brief's contract (`x_min == x_max`) and a mean tolerance tighter than
independent single-precision kernels land — so its records were rescored after
the job was corrected, each keeping the score it replaced.

Each cell reads green/runs · median tokens to green · median tool invocations to
green · median wall clock to green; a silent wrong answer is counted after it.

### 05-gpu-heat

| Model | miri-skill | miri-grammar | miri-both | python | rust |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 999k · 23 · 149s | 3/3 · 1044k · 22 · 179s | 3/3 · 831k · 18 · 194s | 3/3 · 215k · 8 · 73s | 3/3 · 285k · 14 · 280s |
| claude-opus | 3/3 · 362k · 10 · 162s | 3/3 · 594k · 15 · 304s | 3/3 · 820k · 17 · 281s | 3/3 · 121k · 5 · 60s | 3/3 · 255k · 10 · 118s |
| claude-sonnet | 3/3 · 273k · 9 · 129s | 3/3 · 447k · 12 · 149s | 3/3 · 408k · 11 · 154s | 3/3 · 94k · 4 · 33s | 3/3 · 151k · 6 · 79s |

### 06-gpu-life

| Model | miri-skill | miri-grammar | miri-both | python | rust |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 1035k · 22 · 266s | 2/3 · 1143k · 24 · 172s · 1 silent | 3/3 · 1148k · 21 · 234s | 3/3 · 469k · 19 · 234s | 2/3 · 179k · 22 · 335s |
| claude-opus | 3/3 · 360k · 10 · 113s | 3/3 · 522k · 13 · 64s | 3/3 · 503k · 11 · 92s | 3/3 · 121k · 5 · 60s | 3/3 · 281k · 11 · 155s |
| claude-sonnet | 3/3 · 307k · 10 · 166s | 3/3 · 409k · 11 · 61s | 2/3 · 177k · 10 · 159s · 1 silent | 3/3 · 140k · 6 · 41s | 3/3 · 177k · 7 · 82s |

### 07-gpu-escape-time

| Model | miri-skill | miri-grammar | miri-both | python | rust |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 1248k · 25 · 234s | 1/3 · 1016k · 22 · 146s · 2 silent | 3/3 · 1308k · 25 · 171s | 2/3 · 321k · 11 · 107s · 1 silent | 3/3 · 543k · 17 · 212s |
| claude-opus | 3/3 · 415k · 12 · 92s | 3/3 · 518k · 13 · 94s | 3/3 · 550k · 13 · 153s | 3/3 · 179k · 7 · 110s | 3/3 · 257k · 10 · 141s |
| claude-sonnet | 3/3 · 270k · 9 · 38s | 3/3 · 397k · 11 · 47s | 1/3 · 272k · 6 · 34s · 2 silent | 3/3 · 75k · 3 · 29s | 3/3 · 129k · 6 · 89s |

### 08-gpu-histogram

| Model | miri-skill | miri-grammar | miri-both | python | rust |
|---|---|---|---|---|---|
| claude-haiku | 3/3 · 1315k · 29 · 132s | 1/3 · 1350k · 25 · 96s · 2 silent | 3/3 · 1096k · 26 · 113s | 3/3 · 220k · 10 · 60s | 3/3 · 440k · 14 · 140s |
| claude-opus | 3/3 · 615k · 16 · 84s | 3/3 · 672k · 16 · 88s | 3/3 · 658k · 14 · 88s | 3/3 · 98k · 4 · 56s | 3/3 · 198k · 9 · 105s |
| claude-sonnet | 3/3 · 708k · 21 · 73s | 2/3 · 671k · 18 · 56s · 1 silent | 3/3 · 647k · 17 · 60s | 3/3 · 96k · 4 · 39s | 3/3 · 159k · 6 · 84s |

### 09-gpu-particles

| Model | miri-skill | miri-grammar | miri-both | python | rust |
|---|---|---|---|---|---|
| claude-haiku | 2/3 · 871k · 18 · 189s · 1 silent | 3/3 · 1864k · 28 · 328s | 3/3 · 1370k · 25 · 151s | 3/3 · 252k · 9 · 69s | 3/3 · 321k · 12 · 117s |
| claude-opus | 3/3 · 518k · 14 · 77s | 3/3 · 705k · 17 · 97s | 3/3 · 637k · 15 · 88s | 3/3 · 179k · 7 · 75s | 3/3 · 272k · 11 · 156s |
| claude-sonnet | 3/3 · 462k · 14 · 68s | 3/3 · 519k · 14 · 60s | 3/3 · 416k · 9 · 51s | 3/3 · 120k · 5 · 72s | 3/3 · 182k · 7 · 102s |

**Onboarding.** A Miri run's onboarding phase cost a median 120k input tokens,
recorded apart from the numbers above; its context is in them, as cached input
re-read on every measured turn.

**Silent wrong answers are one fault, and the two-phase launch shares it.** All
ten on the Miri arms are runs that wrote their program in a subdirectory
(`job/`, `escape/`, `histogram/`, `particles/`) instead of at `main.mi`: the shell's working
directory, moved during onboarding, carried into the resumed session. A cold arm
cannot meet this. They are scored as the contract reads — no program at
`main.mi` — and the instrument is to name the workspace root in the measured
prompt before another onboarded round.

**Verdicts, as `report.py` computed them.** No claim holds. C1 (`miri-skill`
within 1.5× the cheaper baseline's tokens) fails 15 of 45 comparisons; where it
reached green the skill arm cost 2.2–7.4× Python and 1.3–5.8× Rust. C2 fails one model of three. C3 fails
everywhere: every baseline finished every GPU job, so only the half-cost branch
applies, and the skill arm is never under half. C4 (skill against grammar) fails
3 of 30. C5 (wall clock within 1.5×) fails 10 of 45. Exit criterion: silent wrong
answers fail (the ten above), the pack loop fails 4 of 15, and the opinion
condition is unanswered, since no probe was run.

**The testing loop does not end and the skill pack stays unpublished:** all
three conditions of the exit criterion fail. **The article cannot be written:**
no claim in `CLAIMS.md` holds. Every standing is in `summary.json`: on cost
the skill arm is `behind` both baselines in every cell it reached green; no
comparison reaches `parity`.

## Instrument change, 2026-10-12 (after r2)

The measured prompt names the workspace by its absolute path and tells the
subject to change to it first, and the onboarding text asks for the warm-up to
be written from the workspace without changing into `warmup/`. In r2 every
silent wrong answer on a Miri arm was a run whose resumed session started in
the directory onboarding left it in, so the job's program landed where the
scorer never looks. Cold arms get the same prompt, so the arms stay comparable;
rounds before this change are not, for the onboarded arms' C2 and silent-wrong
numbers.
