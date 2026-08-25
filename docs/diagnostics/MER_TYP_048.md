## Rule

The type checker could not infer a complete type for a variable or expression from the available context. The compiler requires enough information (e.g., explicit type annotations, initialization values, or usage context) to determine an unambiguous type.

## Before

```miri
fn main()
    let x
    println("ok")
```

## After

```miri
fn main()
    let x = 5
    println("ok")
```

## Reference

[Type Checker](../reference/types.md)
