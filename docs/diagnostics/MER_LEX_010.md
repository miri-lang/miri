## Rule

A formatted string (f-string) has invalid syntax. Formatted strings begin with `f"` or `f'` and contain interpolated expressions inside `{...}` braces. An invalid formatted string literal lacks proper quote matching or has structural issues in how the string body is delimited.

## Before

```miri
let x = 5
let msg = f"value = {x
```

## After

```miri
let x = 5
let msg = f"value = {x}"
```

## Reference

[Lexer](../reference/lexer.md)
