# Pre-registered claims

These four claims were written down before the first run of the benchmark, and
the article that reports them links the commit that first carried this file.
Each is falsifiable. The thresholds were fixed on 2026-09-11 and may not be
loosened between rounds; a round in which a claim fails is committed and logged,
not published.

- **C1 — Cost parity with the agent's native languages.** On the CPU jobs, the
  median tokens-to-green for `miri-pack` is within 1.25× of `python` and of
  `typescript`, and below `rust`, per model.
- **C2 — Fewer wrong answers.** Across the CPU jobs, the silent-wrong-answer
  count for `miri-pack` is no higher than any baseline's, where a silent wrong
  answer is a run the agent declared finished that fails a hidden test.
- **C3 — GPU in reach.** On the GPU job, `miri-pack` finishes in every model
  where at least one baseline abandons or hits the turn cap, or its median cost
  is under half the cheapest baseline's.
- **C4 — The pack substitutes for pretraining.** `miri-pack` beats `miri-bare`
  on cost and on finish rate in every job.

## What may not change between rounds

The claims, their thresholds, the briefs, the hidden tests, the caps and the run
count. What may change: the compiler, the pack, and the mechanics of the runner
and the folder. A change to anything in the first list is recorded here with a
dated reason, or it is not made.

`report.py` computes each verdict from the committed records. No verdict is
typed by hand, so a claim cannot be reported as held by the person who wanted it
to hold.
