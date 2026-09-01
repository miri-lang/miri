## Rule

Attributes can be written in two syntaxes: the modern `@name` prefix syntax and a deprecated keyword syntax. This warning is raised when the deprecated syntax is used. Use the `@` prefix instead.

## Messages

- `` the `{keyword}` keyword is deprecated; use `@{replacement}` instead ``

## Before

```miri
must_use enum Status
    Ok
    Error
```

## After

```miri
@must_use
enum Status
    Ok
    Error
```

## Reference

[Type Checker](../reference/types.md)
