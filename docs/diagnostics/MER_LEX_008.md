## Rule

A decimal number literal has invalid underscore placement. Underscores are allowed to separate digit groups (e.g. `1_000_000`) but cannot appear at the start, end, or consecutively. A leading underscore, trailing underscore, or repeated underscores (e.g. `1__0`) are all invalid.

## Before

```miri
let x = 1_
let y = _123
let z = 1__000
```

## After

```miri
let x = 1
let y = 123
let z = 1000
```

## Reference

[Lexer](../reference/lexer.md)
