## Rule

The compiler tracks identifiers that are defined in the current module or imported from elsewhere. When an identifier is used but has not been declared or imported, this error is raised. This error is distinct from a variable name error by also covering undefined types and module members.

## Before

```miri
fn main()
    let x = undefined_identifier
    println(f"{x}")
```

## After

```miri
fn main()
    let x = 42
    println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
