## Rule

A trait definition violates structural requirements. This code covers multiple family members: duplicate method signatures, invalid method declarations, conflicting default implementations, or other constraint violations in trait definitions.

## Before

```miri
trait Reader:
  fn read() string
  fn read() i32

fn main() i32:
  0
```

## After

```miri
trait Reader:
  fn read() string
  fn peek() i32

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
