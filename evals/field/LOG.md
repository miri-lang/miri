# Round log

One entry per round, appended by whoever ran it, newest last. A round in which
any claim fails is logged here in full rather than discarded; the article links
every round it did not publish, with the verdict recorded here.

An entry names: the round, the date, the compiler commit, the model ids, what
moved since the previous round, the verdict per claim as `report.py` computed
it, the ratings its opinion probe collected under `runs/<round>.probe/`, and —
for a re-run under the prompt that drives this benchmark — whether the
milestone's exit criterion holds.

No round has been run against this instrument yet. The numbers that predate it
are in `baseline.md`, and they are not a round.
