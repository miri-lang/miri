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

## Top-Level Statements

Executable statements at the top level of a test file are silently dropped by `miri run`. However, the `miri test` command explicitly rejects such files and refuses to run them:

```miri
use system.testing.{assert_eq}

println("this is a top-level statement")

@test fn my_test()
    assert_eq(1, 1)
```

Running `miri test` on this file produces an error:

```
has executable statements outside a function; move them into a `@test` function
```

All executable code must be inside a `@test fn` or another function. If you need startup code, write a separate `.mi` file with a `fn main()` instead.

## Anti-Hallucination: Test Syntax That Does Not Exist

### Test Functions Must Take No Parameters

Test functions must have zero parameters:

```miri,fails=MER_TYP_017
@test fn bad_test(x int)
    println("test")
```

### Test Functions Must Not Have Return Types

Test functions must have no explicit return type:

```miri,fails=MER_TYP_017
@test fn bad_test() int
    5
```

### Assertions Require an Import

Assertion functions are not in the prelude:

```miri,fails=MER_TYP_034
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
