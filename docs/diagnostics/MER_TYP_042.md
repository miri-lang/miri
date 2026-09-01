## Rule

An assignment was attempted to a variable or field declared as immutable. Variables declared with `let` cannot be reassigned; use `var` to create a mutable binding if reassignment is needed.

## Messages

- `Cannot assign to immutable variable '{var}'`
- `Cannot assign to field of immutable variable`
- `Cannot assign to element of immutable variable`
- `Invalid assignment target`
- `Type mismatch in assignment: cannot assign {actual} to {expected}`
- `expected mutable variable for 'out' parameter '{param}': '{var}' is immutable (declare with 'var')`

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
