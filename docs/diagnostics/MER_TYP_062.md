## Rule

A pattern in a match expression is invalid or malformed. This code covers multiple family members: unreachable patterns, missing required patterns in a guard, type mismatches in pattern bindings, or structural incompatibilities between the matched value and the pattern.

## Messages

- `Pattern type mismatch: expected {expected}, got {got}`
- `Tuple pattern length mismatch: expected {expected}, got {got}`
- `Expected tuple type for tuple pattern, got {type}`
- `Regex pattern requires string subject, got {type}`

## Before

```miri
fn main()
    let code = 5
    match code
        "five": println("five")
        _: println("other")
```

## After

```miri
fn main()
    let code = 5
    match code
        5: println("five")
        _: println("other")
```

## Reference

[Type Checker](../reference/types.md)
