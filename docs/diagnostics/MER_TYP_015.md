## Rule

Some attributes do not accept arguments. When an argument is provided to an attribute that takes none, this error is raised.

## Messages

- `Attribute @{attr} does not take an argument`
- `Remove the argument: @{attr}`

## Before

```miri
@must_use("extra")
enum Result
    Ok
    Err
```

## After

```miri
@must_use
enum Result
    Ok
    Err
```

## Reference

[Type Checker](../reference/types.md)
