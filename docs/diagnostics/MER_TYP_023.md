## Rule

A double negation (`- - x`) is semantically equivalent to a single identity operation (`x`). This warning suggests simplifying the code by removing the redundant negation operator.

## Before

```miri
fn main()
    let x = 5
    let y = - - x
    println(f"{y}")
```

## After

```miri
fn main()
    let x = 5
    let y = x
    println(f"{y}")
```

## Reference

[Type Checker](../reference/types.md)
