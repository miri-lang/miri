# Pre-registered claims

These claims were written down before the first run of the benchmark, and the
article that reports them links the commit that first carried this file. Each
is falsifiable. The thresholds may not be loosened between rounds; a round in
which a claim fails is committed and logged, not published.

Each claim ends by naming the record fields it is measured from and the key
`report.py` publishes its verdict under. A claim stated in terms of something a
record does not carry could only be judged by hand, so the gate refuses one.

The claims are judged over the GPU jobs — every job whose `job.toml` says
`gpu = true` — with Miri measured after onboarding and the baselines cold.
`miri-skill` is the Miri arm the claims are about; `miri-grammar` and
`miri-both` are the onboarding arms it is compared with.

- **C1 — Within reach of the agent's native languages on cost.** On every GPU
  job, per model, the median tokens-to-green for `miri-skill` is at most 1.5
  times the cheaper of `python` and `rust`. A baseline that reached no green run
  leaves `miri-skill` the cheaper by default when it reached one. Measured from
  `tokens`, `outcome`, `hiddenTests`; judged as `C1`.
- **C2 — Fewer wrong answers.** Across the GPU jobs, the silent-wrong-answer
  count for `miri-skill` is no higher than any baseline's, where a silent wrong
  answer is a run the agent declared finished that fails a hidden test or never
  put its work on the GPU. Measured from `silentWrongAnswer`; judged as `C2`.
- **C3 — GPU in reach.** On every GPU job, `miri-skill` finishes in every model
  where at least one baseline abandons or hits the turn cap, or its median cost
  is under half the cheapest baseline's. Measured from `tokens`, `outcome`,
  `hiddenTests`; judged as `C3`.
- **C4 — The pack earns its place.** On every GPU job, per model, `miri-skill`
  is cheaper than `miri-grammar` and finishes at least as often: teaching from
  the skill pack beats teaching from the grammar alone. `miri-both` is reported
  beside it with its standing against `miri-skill`, as a label. Measured from
  `tokens`, `outcome`, `hiddenTests`; judged as `C4`.
- **C5 — Within reach of the agent's native languages on speed.** On every GPU
  job, per model, the median wall-clock-to-green for `miri-skill` is at most 1.5
  times the faster of `python` and `rust`. Measured from `wallClockSeconds`,
  `outcome`, `hiddenTests`; judged as `C5`.

Every comparison C1 or C5 makes against a baseline is still reported with its
standing: `beats`, `parity` (no more than 1.25× the baseline) or `behind`. The
standing is a label, never a verdict — it shows how far a round is from a lead.

## Changes

- **2026-09-11** — C1 to C4 fixed. C1 then claimed parity: within 1.25× of
  `python` and `typescript`, below `rust`.
- **2026-09-14, before any round had run** — C1 tightened from parity to a lead
  over all three baselines, and C5 added. The goal the article reports on is
  that an agent writes Miri faster and cheaper than the languages it already
  knows; a parity claim could hold in a round that shows neither, and speed was
  recorded but judged by nothing. Both changes make the claims harder to hold,
  and no record existed that either could have been fitted to.
- **2026-09-14, before any round had run** — C4's finish-rate half relaxed from
  strictly higher than `miri-bare` to no lower. A bare arm that finishes every
  run leaves no rate to beat, so the strict form could fail every round on the
  bare arm's success rather than the pack's shortfall, and the article gate
  would never open. The cost half stays strict.
- **2026-10-12, before round r2, the user's decision** — the claims are
  re-registered for a different experiment. Round r1 (2026-10-11, in `LOG.md`)
  measured a cold start on CPU jobs and showed C1 and C5 out of reach of any
  tooling: the native baselines sat at the floor, about 4.8 calls a run, no
  lookups and almost no compile errors, an advantage that lives in the model's
  weights. The question is now GPU compute written by agents, the domain Miri is
  designed for, with Miri taught before the job (the onboarding arms) and the
  baselines cold. C1 and C5 become a bar of 1.5× the cheaper (faster) baseline
  instead of a lead, with standings still reported; C2 and C3 keep their form
  over the GPU jobs; C4 compares the onboarding arms, since the cold `miri-pack`
  and `miri-bare` arms are not part of the round. The TypeScript baseline was
  dropped the day before. One brief sentence changes with them: the heat job's
  Miri toolchain line said every `forall` lowers to a GPU kernel, which is not
  so for a bare `forall` over host data, and now says what the newer GPU jobs
  say. r1's verdicts stand as the cold-start result and are not refolded under
  these claims. No record of the new round existed when this was written.

## What may not change between rounds

The claims, their thresholds, the briefs, the hidden tests, the caps and the run
count. What may change: the compiler, the pack, and the mechanics of the runner
and the folder. A change to anything in the first list is recorded above with a
dated reason, or it is not made.

`report.py` computes each verdict from the committed records. No verdict is
typed by hand, so a claim cannot be reported as held by the person who wanted it
to hold.
