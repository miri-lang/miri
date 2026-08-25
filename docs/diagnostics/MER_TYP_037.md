## Rule

A generic type argument does not satisfy the constraint declared for that generic parameter. The type checker verifies that all type arguments conform to their parameter constraints at instantiation sites.

## Before

```miri
class Box<T: Iterable>:
  item T

fn main() i32:
  let b = Box<i32>(5)
  0
```

## After

```miri
class Box<T: Iterable>:
  item T

fn main() i32:
  let b = Box<[i32]>([1, 2, 3])
  0
```

## Reference

[Type Checker](../reference/types.md)
