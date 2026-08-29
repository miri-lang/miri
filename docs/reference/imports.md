# Imports and Module Loading

The module system resolves and loads external `.mi` files via `use` statements. Each import is type-checked independently, and the names it exports are added to the visible scope. Imports are cycle-detected and shadowing is prevented by renaming or selective import.

## What It Rejects

- Circular imports (module A imports B which imports A)
- Name conflicts (an imported name matches a locally declared type)
- Names not found in a module (selective import requests a non-existent export)
- Missing or unreadable module files

## Module Search

Modules are located via dot-notation paths (e.g., `system.io`), with dots converted to directory separators. Paths cannot contain `/`, `\`, or `..`.

The resolution order depends on the import type:

### Bare Imports (`use util`, `use system.io`)

Searched in this order:
1. **Standard library roots**:
   - `MIRI_STDLIB_PATH` environment variable (if set)
   - Directories next to the compiled `miri` binary (`./stdlib`)
   - Install prefix paths (`<prefix>/lib/miri/stdlib`, `<prefix>/share/miri/stdlib`)
   - Manifest directory at build time (`src/stdlib` relative to the repository root)
2. **Project root**: The directory containing the entry file (the `.mi` file passed to `miri run`/`miri check`)
3. **Current working directory**: The directory from which the compiler was invoked

Standard library modules (like `system.io`) cannot be shadowed by user modules. If a bare module name exists in both the stdlib and the project root, the stdlib version is used.

### Local Imports (`use local.utils`, `use local.models.user`)

Resolved only against the **project root** (the entry file's directory), so the working directory does not affect resolution. This makes `use local.utils` resolve the same way regardless of where the compiler is invoked from.

## Per-Code Detail

Use `miri explain MER_IMP_<code>` for detailed guidance on each import diagnostic code. Use `miri explain MER_NAM_<code>` for module-path errors.
