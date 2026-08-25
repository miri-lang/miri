## Rule

A value passed to a collection constructor does not match the element type declared for that collection. All elements must be type-compatible with the collection's generic parameter.

## Before

```miri
fn main() i32:
  let list = [1, 2, "three"]
  0
```

## After

```miri
fn main() i32:
  let list = [1, 2, 3]
  0
```

## Reference

[Type Checker](../reference/types.md)
