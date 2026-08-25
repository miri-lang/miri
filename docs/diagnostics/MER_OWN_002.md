## Rule

A linear variable (a type parameter or generic-bounded variable marked for linear consumption) must be used exactly once. Linear variables cannot be used multiple times, dropped without use, or left unconsumed at the end of a function. They model unique ownership.

## Before

```miri
fn process[T extends Linear](x T) void
    return
```

## After

```miri
fn process[T extends Linear](x T) void
    consume(x)
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
