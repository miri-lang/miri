## Rule

A constant declaration is missing its required initializer. Constants must be declared with an explicit value using the `const` keyword followed by a name and an `=` sign with an expression. A const without an initializer is invalid.

## Messages

- `Constant '{name}' must be initialized with a value`

## Help

- `Add '= <value>' after the constant name, e.g. 'const X = 1'.`

## Before

```miri
const MAX_SIZE
const PI = 3.14
```

## After

```miri
const MAX_SIZE = 100
const PI = 3.14
```

## Reference

[Parser](../reference/parser.md)
