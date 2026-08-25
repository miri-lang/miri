## Rule

A division operation was attempted with a zero divisor at runtime. The operation cannot complete because division by zero is mathematically undefined. Guard against zero values before dividing.

## Before

```miri
fn main()
    let x = 10
    let y = 0
    let result = x / y
```

## After

```miri
fn main()
    let x = 10
    let y = 0
    if y != 0
        let result = x / y
        println("division succeeded")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
