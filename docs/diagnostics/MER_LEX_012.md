## Rule

An expression inside a formatted string's interpolation braces is malformed. The lexer expects braces in a formatted string to be balanced: each opening `{` must have a matching closing `}`, and the code between them must be parseable as a valid Miri expression. Unmatched braces or invalid syntax within the interpolation causes this error.

## Messages

- `Invalid Formatted String Expression`

## Before

```miri
let x = 5
let msg = f"value = {x + "
```

## After

```miri
let x = 5
let msg = f"value = {x + 1}"
```

## Reference

[Lexer](../reference/lexer.md)
