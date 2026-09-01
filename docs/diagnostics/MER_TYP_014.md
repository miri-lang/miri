## Rule

Some attributes require a string literal argument (e.g., `@deprecated("reason")`). This error is raised when such an attribute is used without its required argument.

## Messages

- `Attribute @{attr} requires a string literal argument`
- `Provide an argument: @{attr}("value")`

## Before

```miri
@test
@ignore
fn bad_test()
    println("test")
```

## After

```miri
@test
@ignore("flaky test")
fn good_test()
    println("test")
```

## Reference

[Type Checker](../reference/types.md)
