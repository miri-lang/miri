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
| b | repair a broken program using check, explain and fix | yes | 6 | 1933 | 74 |
| c | add a function and its test | yes | 4 | 702 | 199 |
| d | extend a program with a stdlib module | yes | 4 | 1401 | 85 |
| e | recover from a capability rejection | yes | 5 | 1697 | 118 |
| f | make a failing test pass | yes | 4 | 631 | 128 |
