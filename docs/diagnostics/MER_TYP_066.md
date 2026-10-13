## Rule

An `out` parameter was used incorrectly. An `out` parameter receives a value the callee writes, so the caller must pass a distinct mutable variable for each one: passing the same variable to two `out` parameters makes both writes land in one place. An override must also declare `out` on the same parameters as the method it overrides.

## Messages

- `same variable passed twice as 'out': '{name}' appears more than once`
- `Method '{method}' has incompatible 'out' modifier for parameter '{param}' (position {position}): parent declares {parent}, child declares {child}`

## Before

```miri
fn split(n int, high out int, low out int)
    high = n / 10
    low = n % 10

fn main()
    var digit = 0
    split(42, digit, digit)
    println(f"{digit}")
```

## After

```miri
fn split(n int, high out int, low out int)
    high = n / 10
    low = n % 10

fn main()
    var tens = 0
    var ones = 0
    split(42, tens, ones)
    println(f"{tens} {ones}")
```

## Reference

[Type Checker](../reference/types.md)
