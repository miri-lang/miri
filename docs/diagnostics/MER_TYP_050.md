## Rule

A `break`, `continue`, or `return` statement was used in an invalid context (e.g., `break` outside a loop). These control flow keywords have strict placement rules and can only appear in certain syntactic positions.

## Before

```miri
fn main() i32:
  break
  0
```

## After

```miri
fn main() i32:
  var i = 0
  while i < 5:
    break
  0
```

## Reference

[Type Checker](../reference/types.md)
