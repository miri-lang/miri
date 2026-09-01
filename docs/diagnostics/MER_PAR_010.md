## Rule

A pattern in a `match` expression has already been covered by a previous branch. Each pattern must be unique; duplicate patterns are unreachable and indicate a logic error. Check for repeated literal values, identical identifiers, or overlapping wildcard patterns.

## Messages

- `Duplicate Match Pattern`

## Before

```miri
fn main()
    match 1
        1: println("one")
        1: println("one again")
```

## After

```miri
fn main()
    match 1
        1: println("one")
        2: println("two")
```

## Reference

[Parser](../reference/parser.md)
