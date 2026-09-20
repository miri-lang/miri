## Rule

An integer literal in the source code exceeds the valid range for the inferred or declared integer type. The type checker compares each integer literal against the bit width of the type it is
written into (e.g., `i32`, `i64`) and rejects values outside the representable range.
A literal nothing types is judged against the default `int`, and the message says so;
one written into a named integer type is judged against that type, which is what the
message then names.

## Messages

- `Integer literal '{value}' is out of range for {type} (max {max})`
- `Integer literal '{value}' is out of range for the default int type (i64, max {max})`

## Before

```miri
fn main()
    let x i64 = 9223372036854775808
    println(f"{x}")
```

## After

```miri
fn main()
    let x i64 = 9223372036854775807
    println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
