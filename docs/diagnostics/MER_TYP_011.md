## Rule

When matching on an enum declared with `@non_exhaustive` from outside its defining module, a `default:` arm must be present. This ensures the code remains compatible if the enum gains new variants in the future.

## Messages

- `` Match on `@non_exhaustive` enum '{enum}' requires a `default` arm outside its defining module '{module}' ``

## Before

```miri
use system.os

fn main()
    let err = EnvError.InvalidName("test")
    match err
        EnvError.InvalidName(s): println(s)
        EnvError.InvalidValue(s): println(s)
        EnvError.Other(s): println(s)
```

## After

```miri
use system.os

fn main()
    let err = EnvError.InvalidName("test")
    match err
        EnvError.InvalidName(s): println(s)
        EnvError.InvalidValue(s): println(s)
        EnvError.Other(s): println(s)
        default: println("unknown")
```

## Reference

[Type Checker](../reference/types.md)
