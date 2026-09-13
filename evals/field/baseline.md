# What the third field test measured, 2026-09-09

This is the history the instrument replaced, kept because a benchmark that
starts by discarding its own past numbers can claim any improvement it likes.

Seven blind trials, paired: three jobs each run twice — once with the full agent
surface and the installed skill pack, once with `run` and `build` alone and the
same prose as a static file — plus a seventh trial on a GPU kernel. Models were
mixed deliberately. Every subject was barred from this repository.

| Job | Model | Full surface | `run`/`build` only |
|---|---|---|---|
| Write from scratch | Sonnet | 53+, abandoned | 20, finished |
| Repair six faults | Haiku | 9 | 6 |
| Extend a tracker | Opus | 50 | 13 |
| GPU kernel | Sonnet | 26 | — |

The numbers are **invocations**, not tokens.

## The caveats, recorded beside the numbers

Two effects inflate the tooled column, and a later baseline must not carry
either:

1. **Per-tool verdicts.** The tooled briefs asked each trial to rate the
   surfaces it used. That bought invocations a real job would never spend. A
   measured arm is now never asked for a per-tool verdict; opinions are
   collected afterwards, in a separate probe that is not measured.
2. **Defect-cornering.** Both long tooled runs spent the bulk of their budget
   cornering compiler defects rather than writing code. Those invocations are
   now counted separately from the clean loop, because a compiler defect is a
   fact about the compiler, not about the agent surface wrapped around it.

Netting both out, the two loops cost about the same. That is still the finding
the milestone had to absorb: its premise was that the agent surface makes the
loop *cheaper*, and on three jobs across three models it did not.

## Why these numbers are not a baseline for the current arms

They are **not comparable** to the arms this directory defines. The arms
differ — the pair measured then was a full surface against a restricted one,
both with prose; the pair measured now is `miri-pack` against `miri-bare`, and
`miri-bare` has no prose file at all. The jobs differ, the metric differs
(tokens rather than invocations), and there was no language axis. Nothing in
this table may be quoted next to a number from `runs/`.

They stay here as the Miri-only history under their old arm names, and as the
record of why the instrument was frozen.
