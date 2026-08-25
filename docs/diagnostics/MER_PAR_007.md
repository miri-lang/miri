## Rule

A string literal could not be parsed. The lexer tokenized the input as a string, but parsing the token's content failed. This may occur if the string contains invalid escape sequences or other structural issues.

## Before

```miri
let msg = "incomplete escape \
```

## After

```miri
let msg = "complete string"
```

## Reference

[Parser](../reference/parser.md)
