## Rule

A struct literal is missing some of its required fields. When creating a struct instance with `StructName { ... }`, all fields declared in the struct definition must be provided with values. Missing or omitted fields trigger this error.

## Messages

- `Missing Struct Members`

## Before

```miri
struct Empty
```

## After

```miri
struct Empty
    value i32
```

## Reference

[Parser](../reference/parser.md)
