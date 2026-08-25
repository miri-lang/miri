## Rule

The parser reached the end of the file unexpectedly. The token stream terminated before the parser had completed parsing a construct. This commonly occurs when a block, expression, or statement is left unclosed, or when a required token is missing at the end of the file.

## Before

```miri
let x = (
```

## After

```miri
let x = (5)
```

## Reference

[Parser](../reference/parser.md)
