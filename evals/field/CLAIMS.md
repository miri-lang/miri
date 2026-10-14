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

- **cost-within-reach — Within reach of the agent's native languages on cost.** On every GPU
  job, per model, the median tokens-to-green for `miri-skill` is at most 1.5
  times the cheaper of `python` and `rust`. A baseline that reached no green run
  leaves `miri-skill` the cheaper by default when it reached one. Measured from
  `tokens`, `outcome`, `hiddenTests`; judged as `cost-within-reach`.
- **fewer-wrong-answers — Fewer wrong answers.** Across the GPU jobs, the silent-wrong-answer
  count for `miri-skill` is no higher than any baseline's, where a silent wrong
  answer is a run the agent declared finished that fails a hidden test or never
  put its work on the GPU. Measured from `silentWrongAnswer`; judged as `fewer-wrong-answers`.
- **gpu-in-reach — GPU in reach.** On every GPU job, `miri-skill` finishes in every model
  where at least one baseline abandons or hits the turn cap, or its median cost
  is under half the cheapest baseline's. Measured from `tokens`, `outcome`,
  `hiddenTests`; judged as `gpu-in-reach`.
- **pack-earns-its-place — The pack earns its place.** On every GPU job, per model, `miri-skill`
  is cheaper than `miri-grammar` and finishes at least as often: teaching from
  the skill pack beats teaching from the grammar alone. `miri-both` is reported
  beside it with its standing against `miri-skill`, as a label. Measured from
  `tokens`, `outcome`, `hiddenTests`; judged as `pack-earns-its-place`.
- **speed-within-reach — Within reach of the agent's native languages on speed.** On every GPU
  job, per model, the median wall-clock-to-green for `miri-skill` is at most 1.5
  times the faster of `python` and `rust`. Measured from `wallClockSeconds`,
  `outcome`, `hiddenTests`; judged as `speed-within-reach`.

Every comparison cost-within-reach or speed-within-reach makes against a baseline is still reported with its
standing: `beats`, `parity` (no more than 1.25× the baseline) or `behind`. The
standing is a label, never a verdict — it shows how far a round is from a lead.

## Changes

- **2026-09-11** — cost-within-reach to pack-earns-its-place fixed. cost-within-reach then claimed parity: within 1.25× of
  `python` and `typescript`, below `rust`.
- **2026-09-14, before any round had run** — cost-within-reach tightened from parity to a lead
  over all three baselines, and speed-within-reach added. The goal the article reports on is
  that an agent writes Miri faster and cheaper than the languages it already
  knows; a parity claim could hold in a round that shows neither, and speed was
  recorded but judged by nothing. Both changes make the claims harder to hold,
  and no record existed that either could have been fitted to.
- **2026-09-14, before any round had run** — pack-earns-its-place's finish-rate half relaxed from
  strictly higher than `miri-bare` to no lower. A bare arm that finishes every
  run leaves no rate to beat, so the strict form could fail every round on the
  bare arm's success rather than the pack's shortfall, and the article gate
  would never open. The cost half stays strict.
- **2026-10-12, before the 2026-10-12 round, the user's decision** — the claims are
  re-registered for a different experiment. The 2026-10-11 round (in `LOG.md`)
  measured a cold start on CPU jobs and showed cost-within-reach and speed-within-reach out of reach of any
  tooling: the native baselines sat at the floor, about 4.8 calls a run, no
  lookups and almost no compile errors, an advantage that lives in the model's
  weights. The question is now GPU compute written by agents, the domain Miri is
  designed for, with Miri taught before the job (the onboarding arms) and the
  baselines cold. cost-within-reach and speed-within-reach become a bar of 1.5× the cheaper (faster) baseline
  instead of a lead, with standings still reported; fewer-wrong-answers and gpu-in-reach keep their form
  over the GPU jobs; pack-earns-its-place compares the onboarding arms, since the cold `miri-pack`
  and `miri-bare` arms are not part of the round. The TypeScript baseline was
  dropped the day before. One brief sentence changes with them: the heat job's
  Miri toolchain line said every `forall` lowers to a GPU kernel, which is not
  so for a bare `forall` over host data, and now says what the newer GPU jobs
  say. The 2026-10-11 round's verdicts stand as the cold-start result and are not refolded under
  these claims. No record of the new round existed when this was written.

## What may not change between rounds

The claims, their thresholds, the briefs, the hidden tests, the caps and the run
count. What may change: the compiler, the pack, and the mechanics of the runner
and the folder. A change to anything in the first list is recorded above with a
dated reason, or it is not made.

`report.py` computes each verdict from the committed records. No verdict is
typed by hand, so a claim cannot be reported as held by the person who wanted it
to hold.
