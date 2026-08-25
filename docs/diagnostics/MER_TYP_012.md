## Rule

Attributes are a closed set of identifiers prefixed with `@`. The compiler defines a fixed list of valid attributes (`@test`, `@non_exhaustive`, `@must_use`, etc.). Using any other attribute name is rejected.

## Before

```miri
@nonexistent
enum Status
    Ok
    Error
```

## After

```miri
@must_use
enum Status
    Ok
    Error
```

## Reference

[Type Checker](../reference/types.md)
