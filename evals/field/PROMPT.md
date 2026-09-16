# The prompt that drives a round

This file is the experiment. Every round of the live-model benchmark is
launched by handing an agent the block below, unedited — not by typing a fresh
instruction into a chat window.

That is the whole reason it is committed. Three earlier rounds were each
launched from a hand-typed message, and each one quietly designed a new
experiment: different jobs, different arms, a different metric. Two of them
produced headline numbers that ran in opposite directions, and neither refuted
the other, because they had not measured the same thing. A prompt that lives in
version control cannot drift without the drift appearing in a diff.

Editing the block is allowed and is how the benchmark evolves. Editing it
*while running it* is not: a round is launched from the committed text at the
commit it was launched from, and `LOG.md` records that commit.

## The block

```text
Re-run the frozen agent-loop benchmark. Do not design a new experiment.

1. Read evals/field/README.md. It defines the jobs, the briefs, the planted
   faults, the arms and the model columns. evals/field/CLAIMS.md holds the
   pre-registered claims and their thresholds; neither may be loosened to fit
   a result.

2. Run every cell exactly as specified: briefs verbatim, the models pinned in
   models.toml, subjects barred from the compiler's own repository, scratch
   directories outside it. A model column whose id is empty needs --model-id
   before it can run; a round whose model moved under it is not a round.

       python3 evals/field/bench.py --job <job> --arm <arm> \
           --model <model> --runs 3 --round <round>

   A measured arm is never asked for a per-tool verdict. Asking a subject
   what it thought of a tool buys invocations a real job would not spend.
   Opinions are collected afterwards, by re-running one cell per arm as an
   unmeasured probe whose records are not written into the round:

       python3 evals/field/bench.py --job <job> --arm <arm> \
           --model <model> --round <round> --probe

   Its ratings land in runs/<round>.probe/ and are committed there.

3. Record per cell what bench.py records: tokens, turns, tool invocations,
   toolchain invocations, wall clock, outcome, hidden-test pass rate, and
   whether the run was a silent wrong answer.

   Invocations lost to compiler defects are counted separately from the clean
   loop, as a diagnostic: a hand pass over each transcript under runs/, with
   the count in the round's log entry. It tells the next round what to fix and
   decides no verdict — every claim and the exit criterion are computed from
   gross numbers, because a user of the language pays for its defects too. A
   defect is a fact about the compiler, not about the surface wrapped around
   it; folding the two together is what made two earlier rounds disagree.

   Reproduce every defect claim against the release binary before recording
   it. A claim that does not reproduce is recorded as not reproducing.

4. Fold the round and read the verdicts off the data, never off an opinion:

       python3 evals/field/report.py --round <round>

   The deliverable is the delta table against the previous round's
   summary.json under runs/. The first round has no predecessor and
   establishes the table instead.

   baseline.md holds the numbers that predate this instrument. They were
   measured on different arms, different jobs and a different metric, so they
   are never rewritten, never updated, and never quoted beside a number from
   runs/.

5. New findings follow the binding rules. A finding in a class an earlier task
   already closed is a gate-coverage defect on that task: reopen it and widen
   its gate rather than filing a fresh one. A new task names the mechanical
   gate that makes re-finding its class impossible, not only the test that
   pins the instance. A finding that does not change an agent's outcome is
   recorded as cosmetic and left unnumbered. A compiler defect that produces a
   silent wrong answer outranks everything else found in the round.

6. Append one paragraph per job to evals/field/LOG.md, naming the round, the
   date, the compiler commit, the model ids, what moved since the previous
   round, and the verdict per claim as report.py computed it.

7. Then state plainly, in that same entry, whether the testing loop ends and
   the skill pack is published. It ends on the first round in which all three
   hold:

   - no new silent wrong-answer compiler defect was found;
   - the packed Miri arm needs no more tool invocations to reach green than
     the bare Miri arm, in every job. Measured from `toolInvocations`,
     `outcome`, `hiddenTests`; judged as `packLoop`;
   - no surface the published page recommends was rated 2 out of 5 or lower
     by the unmeasured opinion probe. Read from the `ratings` of the records
     under runs/<round>.probe/; quote every rating of 2 or lower in the entry.

   If any of the three fails, say which, and say that the pack stays
   unpublished. Findings below the silent-wrong-answer bar do not restart the
   loop.

8. Say, in the same entry, whether the article can be written: it reports the
   first round in which report.py holds every claim in CLAIMS.md, and links
   every round before it. For each claim that fails, quote the standings
   report.py gives, so the entry shows how far the round is from a lead.
```

## What the operator provides

`bench.py` needs the harnesses on the path (`claude`, `gemini`), each arm's
toolchain, and a `miri` on the path built from the commit under test —
`cargo build --release` and then that binary, not a stale one. `MIRI_STDLIB_PATH`
must point at the compiler's standard library so a subject can compile without
reading the repository it is barred from.

A round is 5 jobs × 5 arms × 3 models × 3 runs. It costs money and hours, and
nothing in continuous integration runs it.
