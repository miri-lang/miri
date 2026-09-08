## Rule

An import brings in a name nothing in the file uses. The `use` line costs a module load and tells a reader the file depends on something it does not.

Remove the line. There is no spelling that keeps an unused import deliberately: an import that is never used means the same as no import at all, so the fix is always to delete it.

A wildcard import (`use module.*`) is never reported — what it brings in is whatever the module happens to declare, so nothing in the file names it and a report would fire on every one.

## Messages

- `Unused import: '{name}' is never used in this file`

## Before

```miri
use system.math

fn main()
    println("done")
```

## After

```miri
fn main()
    println("done")
```

## Reference

[Imports and Module Loading](../reference/imports.md)
