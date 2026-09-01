## Rule

A backslash character has been used inside an expression within a formatted string. The interpolated code between `{` and `}` in an f-string must be valid Miri code, but backslashes have special meaning in strings and are not allowed to appear directly in these expressions. Use escape sequences outside the braces instead.

## Messages

- `Backslash in Format String`

## Before

```miri
let path = "C:\\Users\\name"
let msg = f"path = {path\n}"
```

## After

```miri
let path = "C:\\Users\\name"
let msg = f"path = {path}"
println(msg)
```

## Reference

[Lexer](../reference/lexer.md)
