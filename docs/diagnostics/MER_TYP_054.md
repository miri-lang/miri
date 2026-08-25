## Rule

A function declaration has an invalid signature. This covers multiple family members: incorrect parameter types, incompatible return types, invalid generic constraints, or mismatched out-parameter declarations. The signature must be well-formed and consistent with any overrides or implementations.

## Before

```miri
fn add(a int, b int) int
    let x = a + b

fn main()
    println("ok")
```

## After

```miri
fn add(a int, b int) int
    return a + b

fn main()
    println("ok")
```

## Reference

[Type Checker](../reference/types.md)
