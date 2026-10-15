# Benchmark workloads

Each directory holds one workload: the same program written once per language,
and `workload.toml`, its manifest. The rules every port follows, the build and
run lines, and what is measured are in [`../CHARTER.md`](../CHARTER.md).

| File | What it is |
|---|---|
| `<name>.mi` | The Miri port, written as the language skill pack teaches. |
| `<name>.c`, `.cpp`, `.rs`, `.go`, `.py`, `.ts` | The headline languages' ports. |
| `workload.toml` | The parameter at each size (S, M, L), the checksum every port must print for it, and the mechanisms the workload stresses (`tags`). |

A port reads one integer from the file named by its first argument and prints
one checksum line. Given `hot` as a second argument, it runs the kernel five
times untimed and five times timed, prints `hot_ns=<n>` to stderr for each
timed run, and exits non-zero if any run's checksum differs from the first.

Every port carries a `port-checklist` line binding its review to the SHA-256
of the file without that line. Changing a port means a new review and a new
line.

The output-identity gate builds every port with its charter build line and
checks it prints the manifest's checksum, with the Miri ports under the leak
check and the heap guard:

```bash
cargo test --test mod bench_identity                        # size S
MIRI_BENCH_SIZES=s,m,l cargo test --test mod bench_identity # every size
```

A port whose toolchain is not installed is skipped with a line naming it.

The sizes are provisional: they were set from the C port on a developer
workstation, not by a calibration run on the publication machine.
