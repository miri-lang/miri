## Rule

An indentation level does not match any enclosing block. The lexer tracks indentation depth when entering (`Indent` token) and exiting (`Dedent` token) code blocks, and requires that all `Dedent` operations align with a previously seen indentation level. An outdent to a level that was never introduced is a mismatch.

## Before

```miri
fn factorial(n int) int
    if n <= 1
        return 1
      return n * factorial(n - 1)
```

## After

```miri
fn factorial(n int) int
    if n <= 1
        return 1
    return n * factorial(n - 1)
```

## Reference

[Lexer](../reference/lexer.md)
