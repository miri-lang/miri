## Rule

A module cannot be loaded because the file does not exist in any of the searched roots or is not readable. Module resolution searches in this order:

1. **Standard library roots** (searched first): `MIRI_STDLIB_PATH` environment variable, the compiler binary's directory, install prefix paths, and the manifest directory (`src/stdlib` relative to the repository root at build time).
2. **Project root**: The directory containing the entry file (the file passed to `miri run`/`miri check`), searched for user modules.
3. **Current working directory**: The directory from which the compiler was invoked, searched last.

For `use local.module` imports, only the project root is searched. For bare `use module` imports (like `use util` for a sibling file), all roots above are searched, with the standard library taking precedence. 

If the module is in the standard library, ensure the module name matches the stdlib path exactly. If it is a user module, verify the file exists relative to the entry file's directory, and that the working directory does not prevent resolution.

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
