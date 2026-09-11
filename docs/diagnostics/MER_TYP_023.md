## Rule

A double negation (`- - x`) is semantically equivalent to a single identity operation (`x`). This warning suggests simplifying the code by removing the redundant negation operator.

## Messages

- `Unnecessary double negation`

## Help

- `The two negations cancel out. If this is intentional, consider simplifying to just the inner expression.`

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
