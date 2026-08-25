## Rule

A name being imported by a module conflicts with a type declared in the importing program. The compiler cannot determine whether the name refers to the local declaration or the imported one. Rename one of them to make the distinction explicit.

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
