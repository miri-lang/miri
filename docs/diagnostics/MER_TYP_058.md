## Rule

A struct definition violates structural requirements. This code covers multiple family members: duplicate field names, forbidden operations on struct types, or other constraint violations specific to struct definitions.

## Before

```miri
struct Point:
  x i32
  x f32

fn main() i32:
  0
```

## After

```miri
struct Point:
  x i32
  y f32

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
