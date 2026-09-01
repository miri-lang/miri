## Rule

A name being imported by a module conflicts with a type declared in the importing program. The compiler cannot determine whether the name refers to the local declaration or the imported one. Rename one of them to make the distinction explicit.

## Messages

- `Type '{name}' is declared in this program and also provided by '{provider}'. Rename the declaration.`
- `` Name '{name}' conflicts with an existing definition from module '{module}'. Use selective imports with an alias to disambiguate, e.g. `use {module}.{... as ...}`. ``
- `` Name '{name}' conflicts with an existing definition from module '{module}'. Use selective imports to avoid ambiguity, e.g. `use {module}.{...}`. ``

## Before

```miri
use system.result

enum Result
    Ok(i32)
    Err(String)
```

## After

```miri
use system.result

enum HttpResult
    Ok(i32)
    Err(String)
```

## Reference

[Imports and Module Loading](../reference/imports.md)
