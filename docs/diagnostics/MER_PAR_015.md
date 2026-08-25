## Rule

A type expression is required but missing or invalid. In contexts where the parser expects a type (e.g. in a type parameter, type annotation, or generic argument), the input was not a valid type expression.

## Before

```miri
fn test(x)
    return
```

## After

```miri
fn test(x i32)
    return
```

## Reference

[Parser](../reference/parser.md)
