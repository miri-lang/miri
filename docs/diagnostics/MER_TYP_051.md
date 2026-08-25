## Rule

A range (e.g., `1..5` or `Range<i32>`) was used with mismatched element types. The start and end of a range must be of the same numeric type for proper iteration and slicing.

## Before

```miri
fn main() i32:
  let r = 1..5.5
  0
```

## After

```miri
fn main() i32:
  let r = 1..5
  0
```

## Reference

[Type Checker](../reference/types.md)
