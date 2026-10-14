## Rule

A range (e.g., `1..5` or `Range<i32>`) was used with mismatched element types. The start and end of a range must be of the same numeric type for proper iteration and slicing.

## Messages

- `Range types mismatch: {start} and {end}`

## Help

- `` convert the value with `as {type}`, which truncates or rounds a value outside its range ``
- `` use a value of type `{expected}` here, or change the type this place declares to `{found}` ``

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
