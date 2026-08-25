## Rule

A slice operation (e.g., `list[1..5]`) was performed on an invalid type or with invalid range bounds. Slicing requires an integer-typed `Range` and is only valid on sequences (lists, arrays, strings). The range bounds must be non-negative and in valid order.

## Before

```miri
fn main() i32:
  let s = "hello"
  let sub = s[1.5..3.5]
  0
```

## After

```miri
fn main() i32:
  let s = "hello"
  let sub = s[1..3]
  0
```

## Reference

[Type Checker](../reference/types.md)
