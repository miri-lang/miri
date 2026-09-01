## Rule

Functions marked with the `@test` attribute must have no parameters and no return type. They are markers for test discovery, not functions to be called with arguments. This error is raised when a `@test` function declares parameters or a return type.

## Messages

- `Invalid test function signature for '{name}': {reason}`

## Before

```miri
@test
fn bad_test(x int) int
    return x * 2
```

## After

```miri
@test
fn good_test()
    println("test ran")
```

## Reference

[Type Checker](../reference/types.md)
