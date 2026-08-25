## Rule

The parser has exceeded the maximum allowed nesting depth. Expressions and statements in Miri are limited to a maximum recursion depth to prevent stack overflow attacks. Deeply nested code, such as many levels of function calls, nested conditionals, or nested operators, can hit this limit.

## Before

```miri
let x = (((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((5 + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1) + 1)
```

## After

```miri
fn compute() int
    return 5
```

## Reference

[Parser](../reference/parser.md)
