# Test Runner

The `test_runner` module implements `miri test`: it finds `@test` functions in `.mi` files, runs each one, and reports the results.

## Writing a test

A test is a function marked `@test`. It takes no parameters and returns nothing — the type checker enforces both (`E0116`). Assertions come from `system.testing`, so a test file that asserts must import it.

```miri
use system.testing

@test
fn test_adds()
    assert(1 + 1 == 2)

@test
@ignore("flaky on CI")
fn test_sometimes_hangs()
    assert(slow_thing() == 1)

@test
@xfail("nested generics lose their type argument")
fn test_known_bug()
    assert(broken_thing() == 1)
```

`@ignore(reason)` skips a test; it is reported with its reason and never executed. `@xfail(reason)` pins a known bug: the test **runs**, and the suite stays green only while it keeps failing. An `@xfail` test that passes is reported as an unexpected pass and fails the run, so fixing the bug forces the marker's removal in the same change. Both markers require a reason and are valid only alongside `@test` (`E0117`).

## How a test runs

Each file is compiled once, with a dispatcher appended to its source:

```miri
runtime "core" fn miri_rt_args_count() int
runtime "core" fn miri_rt_args_at(index int) String

fn main() int
    if miri_rt_args_count() < 1: return 2
    let selected = miri_rt_args_at(0)
    var matched = 0
    if selected == "test_adds"
        test_adds()
        matched = 1
    if matched == 0: return 3
    return 0
```

Each test is then one subprocess spawn of that binary with the test's name as its only argument. Process isolation is the point: a failing assertion terminates its own process, so it is recorded as one failure and the rest of the run continues.

The dispatcher declares the two argv intrinsics itself rather than importing them, so a test file pulls in no module it did not ask for and the compiler holds no knowledge of the standard library. It is *appended*, never spliced, so every span in the user's own source keeps pointing where it did and compile errors stay truthful.

Two details of `miri_rt_args_at` shape the dispatcher: index 0 is the first argument rather than the executable path, and an out-of-range index terminates the process — hence the count guard before the read.

## Reading the result

Exit status carries the verdict. Zero means the test passed; non-zero means it failed, with the process's stderr captured for the report. A death by signal is reported as a crash with its signal number. The two statuses the dispatcher itself returns — `2` for a missing test name, `3` for an unrecognized one — are deliberately distinct from the `1` a failing assertion produces, so a fault in the runner can never be mistaken for a test result, and a mis-dispatched name cannot quietly report a pass.

Leak checking is deliberately **not** enabled for test subprocesses. Several known leaks live in the standard library rather than in user code, and failing an honest test over one of them would say nothing about the test.

## The structured assertion report

Exit status and stderr say *that* a test failed; they do not say where, or what was compared. The line, the compared values and the user's message would all be trapped inside one prose string, which a tool then has to parse back apart.

So a failing assertion also writes a structured record to the path named by `MIRI_ASSERT_REPORT_PATH`, which the runner sets to a fresh file per spawn. Every field is written as `key:<byte-len>:<raw bytes>` followed by a newline. The length prefix is what makes escaping unnecessary: `expected`, `actual`, `message` and the asserted expression's source text are arbitrary user strings that may hold colons and newlines, and a reader that honours the byte count can never be confused by their contents.

The record is written by the test's own process, which is compiled from the user's source and is therefore **untrusted**. The reader treats it that way: it accepts only a regular file, caps the size, rejects an unknown or repeated key, checks every length against the bytes actually present, and does all of its arithmetic checked. Anything it dislikes is discarded whole. The prose `detail` is always populated regardless, so a missing, truncated or hostile record costs the structured fields and nothing else — the failure is still reported exactly as it would have been before.

## What `miri test` returns

The command's own exit status is distinct from the dispatcher statuses above, which belong to the individual test processes:

| Status | Meaning |
|---|---|
| `0` | Every test passed, was ignored, or failed as its `@xfail` documents |
| `1` | At least one test failed, and every discovered file ran |
| `2` | At least one file was refused, whatever the tests that did run reported |

A refusal outranks a failure because it means tests never ran at all, so the run is incomplete rather than merely red. The JSON envelope's `exitCode` is the same value the process returns; both are computed once, in one place, so the two cannot drift apart.

## Files a test file may not be

Three shapes are refused outright rather than run, because each would otherwise fail silently:

- **Declares its own `main`.** It would collide with the dispatcher's, and codegen would emit a duplicate-symbol dump.
- **Has executable statements outside a function.** Script-mode wrapping is skipped once a `main` exists, so those statements would be dropped without a word.
- **Mentions `@test` but does not parse.** Its tests cannot be collected, and skipping it in silence would report a typo'd test file as "0 tests, ok".

A refused file fails the run and is listed under `not run:` with the reason. A file that neither parses nor mentions `@test` is simply not a test file and is passed over quietly.

## Layout

| File | Responsibility |
|---|---|
| `mod.rs` | Result types and the orchestration `run_tests` |
| `discovery.rs` | Directory walk, marker extraction, the refusal rules |
| `harness.rs` | Dispatcher synthesis and its exit statuses |
| `runner.rs` | Per-file compile, per-test spawn, outcome classification |
| `report.rs` | Pretty and JSON rendering |
