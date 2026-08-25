## Rule

A type declaration is invalid. The parser expects a type to be declared using a valid identifier or type expression. A malformed type name, or a token that cannot represent a type in this context, triggers this error.

## Before

```miri
let x [123]
```

## After

```miri
let x [i32]
```

## Reference

[Parser](../reference/parser.md)
