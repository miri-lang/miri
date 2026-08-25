## Rule

A class definition violates structural requirements. This code covers multiple family members: duplicate method names, invalid member declarations, forbidden inheritance patterns, or other rule violations specific to class syntax and semantics.

## Before

```miri
class MyClass:
  value i32
  value string

fn main() i32:
  0
```

## After

```miri
class MyClass:
  value i32
  extra string

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
