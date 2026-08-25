## Rule

A `match` expression does not cover all possible cases. Every value the match expression could evaluate to must be handled. If there is no catch-all `_` pattern and some values are not explicitly matched, the compiler rejects the match as incomplete.

## Before

```miri
fn main()
    match 1
```

## After

```miri
fn main()
    match 1
        default: println("any")
```

## Reference

[Parser](../reference/parser.md)
