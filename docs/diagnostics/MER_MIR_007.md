## Rule

An assignment target (the left-hand side of `=`) is not a valid location to write to. Valid assignment targets are variables, object fields, and array/list elements. Expressions like function calls or literals cannot be assigned to.

## Before

```miri
fn main()
    42 = 10
```

## After

```miri
fn main()
    var x = 42
    x = 10
```

## Reference

[MIR and Lowering](../reference/mir.md)
