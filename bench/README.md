# CPU benchmark suite

How fast compiled Miri programs run and how much memory they hold, next to the
languages people would otherwise write the same programs in. The rules are in
[`CHARTER.md`](CHARTER.md), frozen before the first measured run, and its
machine-readable twin [`charter.toml`](charter.toml).

| Path | What it is |
|---|---|
| `workloads/` | One directory per workload: a port per language and its manifest. See [`workloads/README.md`](workloads/README.md). |
| `c/table.h` | The structures the C ports share, which C's standard library lacks. |
| `hello/` | A hello-world per language, for the runtime floor (`floor_rss_bytes`, `startup_ns`). |
| `run.py` | The runner. |
| `results/` | What the runner writes. Ignored by git: a run is output, not source. |

## Running it

```bash
make bench-smoke                                    # size S; Miri, C and Rust; five measured runs
make bench-full                                     # every workload, language and size
python3 bench/run.py --only nbody --langs miri,c --sizes s,m --runs 5
```

The runner rebuilds this tree's release compiler and runtime libraries with
`make release` first, so a stale binary is never measured (`--miri <path>`
measures a given compiler instead). It refuses to start from a tree with
uncommitted changes unless `--allow-dirty` is given, and refuses a port whose
review line does not match its text.

Before timing anything it builds every port with its charter build line and
checks it prints its manifest's checksum: a port that does not is recorded as
`mismatch` and never timed. It compiles a fixed probe at `--opt-level 0` and
`3` and refuses to run if the two emit the same machine code. Then, for every
workload and size, it runs each language's warm-up and measured runs
interleaved, one language after another in every round, so drift over the run
lands on all of them alike. Each run is a fresh process, executed directly
with no shell, in an environment holding only `PATH`, `HOME`,
`PYTHONHASHSEED=0` and a pinned `DENO_DIR`. Wall time is taken around fork and
exit, and CPU time and peak memory come from the child's `wait4` resource usage.
A noisy cell is re-run from its warm-up as the charter allows, and every
attempt is kept. Sizes S and M also run in hot mode. Each Miri cell gets one
extra, untimed run with the runtime's allocation counter on.

## What it writes

`results/<date>-<profile>/` holds `env.json`, the machine, operating system
and toolchain versions, and `results.jsonl`, one JSON object per measured
run, schema version 1. Every record carries the charter version and hash, the
run id, the machine profile, the runner commit, the Miri version, the
workload's manifest hash, the language, variant, size and mode, and
`process_wall_ns`, `user_ns`, `sys_ns`, `max_rss_bytes`, `hot_ns` (with every
`hot_ns_samples` line), `floor_rss_bytes`, `startup_ns`, `binary_bytes`,
`exit_code` and `status`: `ok` or `timeout`. A cell that produced no
measurements has one record saying why: `mismatch`, `build-failed`,
`toolchain-missing` or `unsupported`, the last with the manifest's reason
when the port is blocked. Miri's `alloc-count` records carry `allocations`.
A results directory is never overwritten; a second run on the same day gets a
new one.
