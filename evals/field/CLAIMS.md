# Pre-registered claims

These claims were written down before the first run of the benchmark, and the
article that reports them links the commit that first carried this file. Each
is falsifiable. The thresholds may not be loosened between rounds; a round in
which a claim fails is committed and logged, not published.

Each claim ends by naming the record fields it is measured from and the key
`report.py` publishes its verdict under. A claim stated in terms of something a
record does not carry could only be judged by hand, so the gate refuses one.

- **C1 — Cheaper than the agent's native languages.** On every CPU job, the
  median tokens-to-green for `miri-pack` is below that of `python`, of
  `typescript` and of `rust`, per model. Measured from `tokens`, `outcome`,
  `hiddenTests`; judged as `C1`.
- **C2 — Fewer wrong answers.** Across the CPU jobs, the silent-wrong-answer
  count for `miri-pack` is no higher than any baseline's, where a silent wrong
  answer is a run the agent declared finished that fails a hidden test.
  Measured from `silentWrongAnswer`; judged as `C2`.
- **C3 — GPU in reach.** On the GPU job, `miri-pack` finishes in every model
  where at least one baseline abandons or hits the turn cap, or its median cost
  is under half the cheapest baseline's. Measured from `tokens`, `outcome`,
  `hiddenTests`; judged as `C3`.
- **C4 — The pack substitutes for pretraining.** In every job, `miri-pack` is
  cheaper than `miri-bare` and finishes at least as often. Measured from
  `tokens`, `outcome`, `hiddenTests`; judged as `C4`.
- **C5 — Faster than the agent's native languages.** On every CPU job, the
  median wall-clock-to-green for `miri-pack` is below that of `python`, of
  `typescript` and of `rust`, per model. Measured from `wallClockSeconds`,
  `outcome`, `hiddenTests`; judged as `C5`.

A comparison C1 or C5 loses is still reported with its standing: `beats`,
`parity` (no more than 1.25× the baseline) or `behind`. Parity is a label, never
a verdict — it shows how far a round is from the claim, and it is the evidence
the programme reads when deciding whether a lead is out of reach.

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

## What may not change between rounds

The claims, their thresholds, the briefs, the hidden tests, the caps and the run
count. What may change: the compiler, the pack, and the mechanics of the runner
and the folder. A change to anything in the first list is recorded above with a
dated reason, or it is not made.

`report.py` computes each verdict from the committed records. No verdict is
typed by hand, so a claim cannot be reported as held by the person who wanted it
to hold.
