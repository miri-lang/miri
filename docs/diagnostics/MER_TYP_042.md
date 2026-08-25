## Rule

An assignment was attempted to a variable or field declared as immutable. Variables declared with `let` cannot be reassigned; use `var` to create a mutable binding if reassignment is needed.

## Before

```miri
fn main() i32:
  let x = 5
  x = 10
  0
```

## After

```miri
fn main() i32:
  var x = 5
  x = 10
  0
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
