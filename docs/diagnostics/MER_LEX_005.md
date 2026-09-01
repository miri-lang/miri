## Rule

A binary literal must start with the `0b` or `0B` prefix followed by only the digits `0` and `1`, optionally separated by underscores. Any other character, including leading or trailing underscores or invalid binary digits, makes the literal invalid.

## Messages

- `Invalid Binary Literal`

## Before

```miri
let x = 0b1012
let y = 0b_101
let z = 0b101_
```

## After

```miri
let x = 0b1010
let y = 0b101
let z = 0b101
```

## Reference

[Lexer](../reference/lexer.md)
