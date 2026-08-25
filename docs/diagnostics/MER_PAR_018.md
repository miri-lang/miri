## Rule

A runtime function declaration specifies an unknown runtime name. Runtime functions use the `runtime` keyword to declare FFI bindings to a named runtime (e.g. `"core"`). The compiler only recognizes specific runtime names; an unknown or misspelled name causes this error.

## Before

```miri
fn increment(x int) int
    runtime "unknown" fn do_increment(x int) int
    return do_increment(x)
```

## After

```miri
fn increment(x int) int
    runtime "core" fn do_increment(x int) int
    return do_increment(x)
```

## Reference

[Parser](../reference/parser.md)
