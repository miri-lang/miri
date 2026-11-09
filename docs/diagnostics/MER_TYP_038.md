## Rule

A pattern or expression named an enum variant the enum does not declare, or named one in a way the enum does not accept — a variant that does not exist, a constructor given the wrong number of fields, or a guard whose shape the variant cannot take.

The commonest form is a pattern that spells the variant without its enum. A bare name in a match arm is a *binding*: it matches everything and binds the subject to that name, so `Ok(n)` does not mean the `Ok` variant unless `Result.` is written in front of it.

A match must also give every value of its subject an arm, as a statement or as an expression, because a value no arm takes has no result to yield. A `bool` needs `true` and `false` (or a catch-all); a tuple is covered position by position; an integer, float or `String` always needs a catch-all, `_` or an unguarded binding. A guarded arm covers nothing, because its guard can fail.

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
- `Non-exhaustive match on {type}. Missing values: {missing}`
- `Non-exhaustive match on {type}. Missing a catch-all arm`
- `Invalid enum variant name`
- `Invalid enum variant definition`
- `Static method '{name}' has the same name as an enum variant - collision between static method and variant`

## Help

- `write '{enum_name}.{variant}' so the pattern names the variant it matches; a bare name binds a new variable instead.`
- `add an arm for '{missing}'; a guarded arm covers nothing, because its guard can fail.`
- `add a '_' arm for the values the other arms leave out; a guarded arm covers nothing, because its guard can fail.`

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
