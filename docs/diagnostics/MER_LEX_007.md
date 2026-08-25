## Rule

A hexadecimal literal must start with the `0x` or `0X` prefix followed by only the digits `0`–`9` and letters `a`–`f` or `A`–`F`, optionally separated by underscores. Any other character, including leading or trailing underscores or letters outside the hexadecimal range, makes the literal invalid.

## Before

```miri
let x = 0xGGGG
let y = 0x_ABCD
let z = 0xABCD_
```

## After

```miri
let x = 0xFFFF
let y = 0xABCD
let z = 0xABCD
```

## Reference

[Lexer](../reference/lexer.md)
