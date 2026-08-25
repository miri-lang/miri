## Rule

The compiler encountered an expression form that is not yet supported by the MIR lowering phase. This typically occurs with rare or edge-case expression types that the compiler does not generate code for.

## Before

```miri
fn main()
    let x = break
```

## After

```miri
fn main()
    var sum = 0
    while sum < 10
        sum = sum + 1
        break
```

## Reference

[MIR and Lowering](../reference/mir.md)
