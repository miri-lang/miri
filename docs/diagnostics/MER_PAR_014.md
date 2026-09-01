## Rule

An enum literal is missing some of its required variants or initialization. When creating an enum instance or in a context that expects all enum variants to be accounted for, missing variants trigger this error.

## Messages

- `Missing Enum Members`

## Before

```miri
enum Empty
```

## After

```miri
enum Empty
    Value
```

## Reference

[Parser](../reference/parser.md)
