## Rule

A cast operation attempted to convert between incompatible types. Not all type pairs can be cast; certain conversions are forbidden (e.g., casting a class to an unrelated type, or casting to a type that does not support it).

## Before

```miri
fn main() i32:
  let x = "hello"
  let y = x as i32
  0
```

## After

```miri
fn main() i32:
  let x = "5"
  let y = i32(x)
  0
```

## Reference

[Type Checker](../reference/types.md)
