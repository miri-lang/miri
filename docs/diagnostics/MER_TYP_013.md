## Rule

Each attribute is valid only on certain declaration kinds (functions, enums, classes, etc.). When an attribute is placed on a declaration type it does not support, this error is raised. Consult the attribute documentation to learn which declaration kinds are valid.

## Messages

- `Attribute @{attr} is not valid on {target}`
- `Attributes valid on {target}: {list}.`

## Before

```miri
@non_exhaustive
fn foo()
    let x = 1
```

## After

```miri
@non_exhaustive
enum Status
    Ok
    Error
```

## Reference

[Type Checker](../reference/types.md)
