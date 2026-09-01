## Rule

A type name used in the program cannot be resolved by the type checker. The type may be undeclared, or declared in a module that the program has not imported. When the type exists in a known module, the compiler provides a help message naming the module to import.

## Messages

- `Unknown type: {name}`
- `Unknown type '{name}' in type declaration`

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

[Types](../reference/types.md)
