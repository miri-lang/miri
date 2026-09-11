# Agent-loop replay results

What one tool-driven job costs against the current compiler. Every row is
a recorded transcript under `evals/<id>/`, replayed against the real
binary; the numbers are observed by the harness, not reported by the
compiler about itself.

All four measured columns are gated: a run that does not reproduce them
fails. That includes a run that gets *cheaper* — a loop that improves
should update this table in the change that earned it, via
`make evals-bless`.

Wall-clock is deliberately absent. It measures the load on the machine
that ran the suite rather than the cost of the loop, and it would rewrite
this file on every run. The harness prints it to stdout instead.

| Task | What it does | Success | Invocations | Bytes read | Bytes written |
|------|--------------|---------|-------------|------------|---------------|
| a | build hello world from an empty directory | yes | 2 | 131 | 39 |
| b | repair a broken program using check, explain and fix | yes | 6 | 2965 | 74 |
| c | add a function and its test | yes | 4 | 702 | 199 |
| d | extend a program with a stdlib module | yes | 4 | 2013 | 85 |
| e | recover from a capability rejection | yes | 5 | 4105 | 118 |
| f | make a failing test pass | yes | 4 | 631 | 128 |
| g | author a struct, a class and a match over an enum from an empty directory | yes | 3 | 588 | 1339 |
| h | repair a cascade with one root cause | yes | 4 | 2185 | 232 |
| i | look up an API and edit through view and patch only | yes | 4 | 1233 | 97 |
| j | recover from a runtime trap | yes | 5 | 1030 | 233 |
| l | repair a file with four faults, three of which carry a repair | yes | 8 | 6403 | 1292 |
| m | sort a list whose element type has no ordering | yes | 3 | 948 | 679 |
| n | chain a list transform onto another transform's result | yes | 3 | 1262 | 433 |
| o | apply the edit a diagnostic already named in its help | yes | 6 | 3186 | 219 |
| p | repair three syntax faults reported from one check | yes | 4 | 4251 | 362 |
| k | read the warnings a green test run left behind | no | 2 | 1398 | 0 |

Tasks recorded as not succeeding are pinned on a gap the loop still has.
They are replayed to the step that fails and cost what they cost getting
there. Closing one makes it finish, which moves `success` and fails the
gate — so the fix and this table land together.

- **k** (stops at step 2): `miri test` renders warnings to stderr as text and omits them from the envelope
