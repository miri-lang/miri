## Rule

A builtin type constructor (e.g., `i32()`, `List<T>()`) was called with invalid arguments. The constructor's arity or argument types do not match what the builtin type expects, or the constructor was used on a type that does not support construction.

## Before

```miri
fn main()
    let x = i32("not a number")
    println("ok")
```

## After

```miri
fn main()
    let x = i32(42)
    println("ok")
```

## Reference

[Type Checker](../reference/types.md)
