## Rule

A slice operation (e.g., `list[1..5]`) was performed on an invalid type or with invalid range bounds. Slicing requires an integer-typed `Range` and is only valid on sequences (lists, arrays, strings). The range bounds must be non-negative and in valid order.

## Messages

- `Slice range must be of integer type`
- `Type {type} is not sliceable`
- `Cannot slice heterogeneous tuple`
- `Slice start index ({start}) is greater than end index ({end})`
- `slice expects a bounded range argument, e.g. 'g.slice(0..10)'`

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
