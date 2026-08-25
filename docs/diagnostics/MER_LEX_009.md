## Rule

A regex literal must be properly quoted and have a matching closing quote. The lexer requires regex patterns to be enclosed in either single or double quotes (preceded by `re`) and properly terminated. The opening and closing quotes must match, and both must be present.

## Before

```miri
let pattern = re'[a-z]
let other = re"[0-9]'
```

## After

```miri
let pattern = re'[a-z]'
let other = re"[0-9]"
```

## Reference

[Lexer](../reference/lexer.md)
