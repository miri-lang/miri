## Rule

A struct definition violates structural requirements. This code covers multiple family members: duplicate field names, forbidden operations on struct types, or other constraint violations specific to struct definitions. A struct holds data only, so it declares no methods, implements no traits and has no drop hook: give a type that needs any of those a class declaration instead.

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
