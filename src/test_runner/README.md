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
