## Rule

The compiler encountered a statement form that is not yet supported by the MIR lowering phase. This typically occurs with rare or edge-case statement types that the compiler does not translate to MIR.

## Before

```miri
fn main()
    defer println("cleanup")
```

## After

```miri
fn main()
    println("work done")
    println("cleanup")
```

## Reference

[MIR and Lowering](../reference/mir.md)
