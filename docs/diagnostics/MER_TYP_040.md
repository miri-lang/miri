## Rule

An array or list was indexed with a value of an invalid type, or an out-of-bounds constant index was detected. Indices must be integers; the compiler validates constant indices against known collection sizes.

## Before

```miri
fn main() i32:
  let arr = [1, 2, 3]
  let x = arr["invalid"]
  0
```

## After

```miri
fn main() i32:
  let arr = [1, 2, 3]
  let x = arr[0]
  0
```

## Reference

[Type Checker](../reference/types.md)
