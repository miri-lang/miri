## Rule

A multiline comment starting with `/*` has not been closed with `*/`. The lexer requires all multiline comments to be properly terminated. Check for a missing closing `*/` or a nested comment depth mismatch.

## Messages

- `Unclosed Multiline Comment`

## Before

```miri
use system.io

fn main()
    /* This comment starts here
    println("but never closes")
```

## After

```miri
use system.io

fn main()
    /* This comment starts here */
    println("but never closes")
```

## Reference

[Lexer](../reference/lexer.md)
