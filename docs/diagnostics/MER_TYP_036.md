## Rule

A generic type was instantiated with a different number of type arguments than the number of generic parameters it declares. Every generic parameter must be satisfied with a corresponding type argument at instantiation.

## Before

```miri
class Container<T, U>:
  value T

fn main() i32:
  let c = Container<i32>("mismatch")
  0
```

## After

```miri
class Container<T, U>:
  value T
  extra U

fn main() i32:
  let c = Container<i32, string>(5, "match")
  0
```

## Reference

[Type Checker](../reference/types.md)
