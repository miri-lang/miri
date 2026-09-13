# The six planted faults

This file is the maintainer's map, not part of the job: it is never copied into
a subject's directory. It exists so that a port can be checked for carrying the
same faults as the others rather than a translator's approximation of them.

Every fault is **semantic**. None is a syntax error, a type error or anything a
compiler or linter reports, because a fault a toolchain points at measures the
toolchain rather than the agent, and it would measure a different one in each
language.

The anchor is the text the faulted line carries in **every** port. A port that
drops a fault stops holding its anchor, and the structural gate over this
directory fails.

| Fault | What the program does | What the contract asks for | Anchor |
|---|---|---|---|
| F1 | Reads every transaction but the last | Reads all `n` of them | `n - 1` |
| F2 | Adds a debit to the balance | Subtracts it | `debit_sign = 1` |
| F3 | Keeps an amount's whole part and drops its cents | Keeps the amount to the cent | `whole_only * 100` |
| F4 | Orders the report by account name | Orders it by balance descending, ties by account | `by_account` |
| F5 | Writes nothing for a ledger with no transactions | Writes `TOTAL 0.00` | `n == 0` |
| F6 | Counts a transaction of an unknown kind and carries on | Writes `ERROR line <k>` and nothing else | `skipped` |

## Why these six

They cover the classes a repair job has to separate: a boundary (F1), a sign
(F2), a rounding rule (F3), an ordering key (F4), an empty input (F5) and a
missing error path (F6). Four of them are visible in the report the program
already prints; two — the empty ledger and the unknown kind — only appear on an
input the subject has to think to try.

Each is repairable on its own, and repairing all six makes the program satisfy
the contract exactly. That was verified by repairing each port and running it
against every case under `cases/`.
