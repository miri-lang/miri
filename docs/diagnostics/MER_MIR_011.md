## Rule

A type used in an expression or declaration is not supported by the compiler. This occurs with types that the compiler lacks translation rules for during MIR lowering.

## Before

```miri
fn main()
    let x Unsupported = 42
```

## After

```miri
fn main()
    let x = i32(42)
```

## Reference

[MIR and Lowering](../reference/mir.md)
