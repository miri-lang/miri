## Rule

The compiler enforces type compatibility at assignment, return, and function-call sites. When the type of a value does not match the declared or inferred type expected at that location, a type mismatch error is raised.

## Before

```miri
fn main()
    let x int = "hello"
    println(f"{x}")
```

## After

```miri
fn main()
    let x int = 42
    println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
