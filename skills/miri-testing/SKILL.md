---
name: miri-testing
description: Writing Miri tests — miri test runner, test attributes, and assertion functions
---

# Testing in Miri

Miri includes a built-in test runner and assertion library. This skill teaches how to write tests, run them, and understand test attributes.

## Writing and Running Tests

### Basic Test

Mark a function with `@test` to make it a test case:

```miri
use system.testing.{assert_eq}

@test fn simple_add()
    assert_eq(2 + 2, 4)
```

Run tests with the `miri test` command:

```bash
miri test --dir <directory>
```

The runner discovers all `.mi` files, finds functions marked with `@test`, and executes them.

### Assertion Functions

Miri provides four assertion functions (all require an import):

```miri
use system.testing.{assert, assert_eq, assert_ne, assert_panics}

@test fn test_assert()
    assert(true)

@test fn test_equals()
    assert_eq(5, 5)

@test fn test_not_equals()
    assert_ne(3, 4)

@test fn test_panic()
    assert_panics(fn(): panic("expected error"))
```

All assertions accept an optional message argument.

### Test Runner Options

```bash
miri test --dir <DIR>           # Run tests in directory
miri test --filter <PATTERN>    # Run tests matching pattern
miri test --format pretty|json  # Output format
miri test -v                    # Verbose
miri test --verify-mir          # Verify MIR optimization pass
```

## Test File Rules

A test file holds only declarations (types, functions, test functions). It must not declare `fn main` and must not have executable statements outside a function. The `miri test` runner enforces both rules:

**Correct test file:**

```miri
use system.testing.{assert_eq}

@test fn my_test()
    assert_eq(1, 1)

@test fn another()
    assert_eq(2, 2)
```

**Declaring `fn main` is rejected:**

The runner says:
```
declares its own `main`; a test file holds only declarations, and the runner supplies the entry point
```
The runner appends its own `main` dispatcher, so any user-declared `main` causes a conflict.

**Top-level executable statements are rejected:**

The runner says:
```
has executable statements outside a function; move them into a `@test` function, where they would otherwise be silently skipped
```

**Non-parsing test files are rejected:**

The runner says:
```
declares `@test` but does not parse; run `miri check` on it for the syntax error
```
A test file must be valid Miri syntax; files with syntax errors are rejected without running any tests.

**Why these rules have no `fails=` blocks:**

Files with `fn main`, top-level statements, or parse errors type-check cleanly (or fail to parse entirely), so the compiler-driven gate cannot express them as `fails=` blocks. Only the `miri test` runner enforces these rejection rules at runtime.

To run executable setup code outside a test, write a separate `.mi` file with a regular `fn main()` instead.

## Module Resolution in Tests

`miri test --dir <DIR>` works from any working directory. Imports in a test file resolve against the **entry file's directory** (where the test file is located), not the shell's current working directory. Set `MIRI_STDLIB_PATH` to override the stdlib search when the binary is not beside `stdlib/`.

## Anti-Hallucination: Test Syntax That Does Not Exist

### Test Functions Must Take No Parameters

Test functions must have zero parameters:

```miri,fails=MER_TYP_017,expects-message=test functions must take zero parameters
@test fn bad_test(x int)
    println("test")
```

### Test Functions Must Not Have Return Types

Test functions must have no explicit return type:

```miri,fails=MER_TYP_017,expects-message=test functions must not declare a return type
@test fn bad_test() int
    5
```

### Assertions Require an Import

Assertion functions are not in the prelude:

```miri,fails=MER_TYP_034,expects-message=Undefined variable: assert_eq
@test fn test_add()
    assert_eq(2 + 2, 4)
```

Always import from `system.testing`:

```miri
use system.testing.{assert_eq}

@test fn test_add()
    assert_eq(2 + 2, 4)
```

## Test Discovery and Execution

The `miri test` runner scans all `.mi` files in a directory and its subdirectories, finds `@test fn` declarations, and executes each one. Output follows this format:

```
running N tests
test <file>::<function> ... ok
test <file>::<function> ... FAILED

test result: ok. N passed; 0 failed; 0 ignored
```

A test passes if it runs without panicking. Any unhandled panic causes a FAILED result.

## Workflow

1. Write a test function with `@test fn name()` and call assertions.
2. Run `miri test --dir .` to execute all tests.
3. Fix failures by adjusting the code or the assertion logic.
4. Use `--filter <pattern>` to run a subset during development.
