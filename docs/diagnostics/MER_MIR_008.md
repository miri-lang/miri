## Rule

An operator used in an expression is not supported by the compiler. Supported operators include arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`==`, `!=`, `<`, `>`, `<=`, `>=`), and logical (`&&`, `||`) operators.

## Before

```miri
fn main()
    let x = 5
    let y = x >>> 2
```

## After

```miri
fn main()
    let x = 5
    let y = x / 4
```

## Reference

[MIR and Lowering](../reference/mir.md)
