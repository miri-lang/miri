## Rule

A boolean literal could not be parsed. The lexer recognizes `true` and `false` as boolean keywords, but parsing the token as a boolean value in a context that expects one failed. This is rare and usually indicates an internal parsing error.

## Before

```miri
let flag = maybe
```

## After

```miri
let flag = true
```

## Reference

[Parser](../reference/parser.md)
