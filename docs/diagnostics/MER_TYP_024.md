## Rule

Miri does not support the decrement operator. The `--` syntax is parsed as two negation operators, not a decrement. Instead, use explicit subtraction (e.g., `x = x - 1`) to decrement a variable.

## Messages

- `Decrement operator not supported`

## Help

- `` `--x` is parsed as two negations (`-(-x)`), not as a decrement. Miri does not have a decrement operator — use `x = x - 1` instead. ``

## Before

```miri
fn main()
    var x = 10
    let y = --x
    println(f"{y}")
```

## After

```miri
fn main()
    var x = 10
    x = x - 1
    println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
