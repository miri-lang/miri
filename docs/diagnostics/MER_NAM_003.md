## Rule

The import path is syntactically invalid. Paths must use dot notation (`system.collections.list`), contain only alphanumeric characters and underscores in each segment, and cannot contain `/`, `\`, or `..`. The path is extracted from the expression preceding the import statement.

## Before

```miri
use system/io
```

## After

```miri
use system.io
```

## Reference

[Imports and Module Loading](../reference/imports.md)
