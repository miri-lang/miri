## Rule

A cast operation attempted to convert between incompatible types. Not all type pairs can be cast; certain conversions are forbidden (e.g., casting a class to an unrelated type, or casting to a type that does not support it).

## Messages

- `` cannot cast from non-numeric type `{source}` to `{target}` ``
- `` cannot cast from `{source}` to non-numeric type `{target}` ``

## Help

- `` read the number out of the text with `.to_int()` or `.to_float()`, which give `None` when it holds no number ``
- `` `as` converts between numeric types only; convert this value another way ``
- `` format the value as text with an f-string, e.g. `f"{x}"` ``
- `` `as` converts to numeric types only; build the value another way ``

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
