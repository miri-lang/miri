## Rule

An enum definition violates structural requirements. This code covers multiple family members: duplicate variant names, invalid variant syntax, forbidden generic constraints, or other violations specific to enum definitions.

## Before

```miri
enum Color:
  Red
  Red
  Blue

fn main() i32:
  0
```

## After

```miri
enum Color:
  Red
  Green
  Blue

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
