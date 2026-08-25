## Rule

An out-parameter (a parameter marked as `out` to receive a value from the function) was used or declared incorrectly. Out-parameters can only appear in specific contexts; they cannot be read before initialization and must satisfy certain type constraints.

## Before

```miri
fn process(out result i32, out extra string) i32:
  0

fn main() i32:
  process("invalid", "wrong")
```

## After

```miri
fn process(out result i32) i32:
  result = 42
  0

fn main() i32:
  process(0)
```

## Reference

[Type Checker](../reference/types.md)
