## Rule

A variable was declared with the `shared` keyword in a context where it is not valid. `shared` is reserved for specific contexts (e.g., certain module-level declarations) and carries semantics that prevent its use in function-local scopes.

## Before

```miri
fn main() i32:
  shared var x = 5
  0
```

## After

```miri
var x = 5

fn main() i32:
  0
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
