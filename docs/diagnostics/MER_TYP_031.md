## Rule

In function calls that use named arguments, all positional (unnamed) arguments must come before named arguments. This error is raised when a positional argument appears after a named argument.

## Messages

- `Positional arguments cannot follow named arguments`

## Before

```miri
fn takes_args(a int, b int, c int) int
    return a + b + c

fn main()
    let result = takes_args(1, b: 2, 3)

```

## After

```miri
fn takes_args(a int, b int, c int) int
    return a + b + c

fn main()
    let result = takes_args(1, c: 3, b: 2)
```

## Reference

[Type Checker](../reference/types.md)
