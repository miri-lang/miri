## Rule

When a function, class, or enum marked with `@deprecated` is used, this warning is raised to alert the user that the entity is going out of favor and should not be relied upon. The deprecation message provides guidance on what to use instead. Deprecated code still compiles and runs.

## Before

```miri
@deprecated("use new_function instead")
fn old_function() int
    42

fn main()
    let x = old_function()
    println(f"{x}")
```

## After

```miri
fn new_function() int
    42

fn main()
    let x = new_function()
    println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
