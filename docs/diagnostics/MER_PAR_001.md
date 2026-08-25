## Rule

The parser encountered a token that was not expected at this position. A specific token was required to parse the current construct (e.g. an identifier after `fn`, a colon after a type annotation), but a different token was found instead. This is a family code covering dozens of specific parse contexts where the wrong token appears.

## Before

```miri
fn main)
    println("Hello")
```

## After

```miri
fn main()
    println("Hello")
```

## Reference

[Parser](../reference/parser.md)
