## Rule

An invalid character or sequence has been encountered that cannot be tokenized. The lexer recognizes keywords, operators, identifiers, literals, and comments, but rejected this input as not matching any valid token pattern. Check for stray or misplaced characters, or a syntax error in the surrounding code.

## Messages

- `Invalid Token`

## Before

```miri
use system.io

fn main()
    let x = $ 5
    println(x)
```

## After

```miri
use system.io

fn main()
    let x = 5
    println(x)
```

## Reference

[Lexer](../reference/lexer.md)
