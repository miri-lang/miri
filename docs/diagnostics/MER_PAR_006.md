## Rule

A float literal could not be parsed. The lexer tokenized the input as a float, but parsing the token's value as a floating-point number failed. This may occur if the float literal is malformed or has an invalid format.

## Before

```miri
let x = 1.0e999
let y = .
```

## After

```miri
let x = 1.0e308
let y = 0.5
```

## Reference

[Parser](../reference/parser.md)
