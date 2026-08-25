## Rule

A static method was accessed or defined in an invalid context. Static methods belong to the type itself rather than instances, and cannot use `self` or instance state. Attempting to use `self` in a static method or calling an instance method as static triggers this error.

## Before

```miri
class Counter:
  count i32
  
  public static fn reset() i32:
    self.count = 0
    0

fn main() i32:
  0
```

## After

```miri
class Counter:
  count i32
  
  public static fn reset() i32:
    0

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
