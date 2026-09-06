## Rule

A pattern or expression named an enum variant the enum does not declare, or named one in a way the enum does not accept — a variant that does not exist, a constructor given the wrong number of fields, or a guard whose shape the variant cannot take.

The commonest form is a pattern that spells the variant without its enum. A bare name in a match arm is a *binding*: it matches everything and binds the subject to that name, so `Ok(n)` does not mean the `Ok` variant unless `Result.` is written in front of it.

## Messages

- `Expected enum variant pattern like {enum}.{variant}`
- `Expected an enum variant pattern, but the subject has type {type}`
- `Invalid enum variant pattern`
- `Enum '{enum}' has no variant '{variant}'`
- `Enum variant '{variant}' expects {expected} bindings, got {actual}`
- `Enum variant '{enum}.{variant}' expects {expected} arguments, got {actual}`
- `Type mismatch in enum variant '{enum}.{variant}': expected {expected}, got {actual}`
- `Non-exhaustive match on Enum '{enum}'. Missing variants: {missing}`
- `Non-exhaustive match on Option. Missing variants: {missing}`
- `Invalid enum variant name`
- `Invalid enum variant definition`
- `Static method '{name}' has the same name as an enum variant - collision between static method and variant`

## Before

```miri
fn get() Result<int, String>
    Result.Ok(1)

fn main()
    match get()
        Ok(n): println(f"{n}")
        Result.Err(e): println(e)
```

## After

```miri
fn get() Result<int, String>
    Result.Ok(1)

fn main()
    match get()
        Result.Ok(n): println(f"{n}")
        Result.Err(e): println(e)
```

## Reference

[Type Checker](../reference/types.md)
