## Rule

A constant expression was evaluated at compile time and resulted in an invalid arithmetic operation. This includes division by zero, overflow in integer arithmetic, or other errors that prevent evaluation of constant expressions.

## Messages

- `Division by zero`

## Before

```miri
let SIZE = 10 / 0

fn main() i32:
  0
```

## After

```miri
let SIZE = 10 / 2

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
