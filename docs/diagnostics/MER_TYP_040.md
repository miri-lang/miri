## Rule

An array or list was indexed with a value of an invalid type, or an out-of-bounds constant index was detected. Indices must be integers; the compiler validates constant indices against known collection sizes.

## Messages

- `String index must be an integer`
- `{type} index must be an integer`
- `Index must be a non-negative integer`
- `Index out of bounds: index {idx} but collection has {size} elements`
- `Tuple index must be an integer`
- `Tuple index out of bounds (empty tuple)`
- `Tuple index out of bounds`
- `Tuple index must be an integer literal for heterogeneous tuples`
- `Type {type} is not indexable`
- `Slice start index must be a non-negative integer`
- `Slice start index out of bounds: index {idx} but array has {size} elements`
- `Slice end index must be a non-negative integer`
- `Slice end index out of bounds: index {idx} but collection has {size} elements`

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
