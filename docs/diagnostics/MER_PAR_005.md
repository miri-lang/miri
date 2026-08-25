## Rule

An integer literal could not be parsed. The lexer tokenized the input as an integer, but parsing the token's value as a number failed. This may occur if the integer literal is malformed in a way the regex did not catch, or if there is an internal parsing error.

## Before

```miri
let x = 170141183460469231731687303715884105728
```

## After

```miri
let x = 100
```

## Reference

[Parser](../reference/parser.md)
