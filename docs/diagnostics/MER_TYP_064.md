## Rule

A variable name shadows a previous declaration in a way that is not allowed. Shadowing rules restrict when a new binding can use a name already in scope; certain contexts forbid redeclaring a name to prevent confusion.

## Before

```miri
fn main() i32:
  let x = 5
  let x = 10
  0
```

## After

```miri
fn main() i32:
  let x = 5
  var y = 10
  0
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
