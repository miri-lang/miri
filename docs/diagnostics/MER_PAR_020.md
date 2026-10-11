## Rule

A logical operator has been spelled with symbols. Miri spells logical and as the word `and` and logical or as the word `or`; `&&` and `||` are not operators. `miri fix` rewrites them (`word-operator`).

## Messages

- `'{operator}' is not a valid operator in Miri`

## Help

- `` Miri spells logical and as the word `and`: `if a and b`. ``
- `` Miri spells logical or as the word `or`: `if a or b`. ``
- `Use '{suggestion}' instead of '{found}'.`

## Before

```miri
let a = true
let b = false
let both = a && b
```

## After

```miri
let a = true
let b = false
let both = a and b
```

## Reference

[Parser](../reference/parser.md)
