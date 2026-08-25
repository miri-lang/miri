## Rule

A remainder (modulo) operation was attempted with a zero divisor at runtime. The remainder operation cannot complete because taking a remainder with a zero divisor is mathematically undefined. Guard against zero values before computing a remainder.

## Before

```miri
fn main()
    let x = 17
    let y = 0
    let result = x % y
```

## After

```miri
fn main()
    let x = 17
    let y = 5
    let result = x % y
    println("modulo succeeded")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
