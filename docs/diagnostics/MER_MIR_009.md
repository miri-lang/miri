## Rule

A loop range type is not supported by the compiler. Loop ranges must be either exclusive (using `..`) or inclusive (using `..=`) with integer types. Other range forms are not supported.

## Before

```miri
fn main()
    for x in "a".."z"
        println(x)
```

## After

```miri
fn main()
    for x in 0..26
        println(x)
```

## Reference

[MIR and Lowering](../reference/mir.md)
