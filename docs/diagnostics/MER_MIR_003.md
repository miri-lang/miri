## Rule

A variable is used in an expression but was never defined in the current scope. This typically occurs when a variable name is misspelled or referenced before its declaration.

## Before

```miri
fn main()
    println(undefined_var)
```

## After

```miri
fn main()
    let defined_var = 42
    println(defined_var)
```

## Reference

[MIR and Lowering](../reference/mir.md)
