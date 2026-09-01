## Rule

An octal literal must start with the `0o` or `0O` prefix followed by only the digits `0` through `7`, optionally separated by underscores. Any other character, including leading or trailing underscores, letters outside the octal range, or digits 8 and 9, makes the literal invalid.

## Messages

- `Invalid Octal Literal`

## Before

```miri
let x = 0o789
let y = 0o_755
let z = 0o755_
```

## After

```miri
let x = 0o755
let y = 0o755
let z = 0o755
```

## Reference

[Lexer](../reference/lexer.md)
