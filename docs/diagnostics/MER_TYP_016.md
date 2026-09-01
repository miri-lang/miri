## Rule

Regex literals use Miri's regex syntax. This error is raised when a regex literal contains an invalid pattern (such as mismatched parentheses, invalid escape sequences, or invalid flags).

## Messages

- `Invalid regex literal: {error}`

## Before

```miri
fn main()
    let pattern = re"[invalid("
    println(f"{pattern}")
```

## After

```miri
fn main()
    let pattern = re"[a-z]+"
    println(f"{pattern}")
```

## Reference

[Type Checker](../reference/types.md)
