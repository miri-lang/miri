## Rule

An import statement references a type that does not exist in the target module. The compiler cannot locate the requested type name in the module's public exports.

## Before


```miri
fn main()
    let value Widget = 1
    println(f"{value}")
```

## After


```miri
fn main()
    let value int = 1
    println(f"{value}")
```

## Reference

[Imports and Module Loading](../reference/imports.md)
