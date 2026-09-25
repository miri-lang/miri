## Rule

An integer literal could not be parsed. The lexer tokenized the input as an integer, but parsing the token's value as a number failed. A decimal literal may be as large as `u128::MAX` (340282366920938463463374607431768211455), the widest integer type; anything larger has no type that can hold it. Whether a literal within that bound fits the type it is written into is a separate check (`MER_TYP_068`).

## Messages

- `Invalid Integer Literal`

## Help

- `Ensure the integer literal format is correct.`

## Before

```miri
let x = 340282366920938463463374607431768211456
```

## After

```miri
let x = 100
```

## Reference

[Parser](../reference/parser.md)
