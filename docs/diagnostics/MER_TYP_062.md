## Rule

A pattern in a match expression is invalid or malformed. This code covers multiple family members: unreachable patterns, missing required patterns in a guard, type mismatches in pattern bindings, or structural incompatibilities between the matched value and the pattern.

## Before

```miri
fn main() i32:
  let x = 5
  match x:
    1: 0
    2: 1
    3: 2
  0
```

## After

```miri
fn main() i32:
  let x = 5
  match x:
    1: 0
    2: 1
    default: 2
  0
```

## Reference

[Type Checker](../reference/types.md)
