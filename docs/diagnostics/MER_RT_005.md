## Rule

An assertion failed at runtime. The program expected a certain condition to be true, but it was false when evaluated. Fix the bug in your program or update the assertion if the expectation was incorrect.

## Before

```miri
use system.testing

fn main()
    let x = 5
    assert(x == 10, "x should be 10")
```

## After

```miri
use system.testing

fn main()
    let x = 5
    assert(x == 5, "x should be 5")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
