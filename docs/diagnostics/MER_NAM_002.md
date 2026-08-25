## Rule

A module cannot be loaded because the file does not exist or is not readable. Import paths use dot notation (`system.io`) and are resolved relative to the module root directory. Verify the module name, path separators, and file permissions.

## Before

```miri
use system.missing_module
```

## After

```miri
use system.io
```

## Reference

[Imports and Module Loading](../reference/imports.md)
