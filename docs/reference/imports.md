# Imports and Module Loading

The module system resolves and loads external `.mi` files via `use` statements. Each import is type-checked independently, and the names it exports are added to the visible scope. Imports are cycle-detected and shadowing is prevented by renaming or selective import.

## What It Rejects

- Circular imports (module A imports B which imports A)
- Name conflicts (an imported name matches a locally declared type)
- Names not found in a module (selective import requests a non-existent export)
- Missing or unreadable module files

## Module Search

Modules are located via dot-notation paths (e.g., `system.io`) resolved against the module root. Paths cannot contain `/`, `\`, or `..`.

## Per-Code Detail

Use `miri explain MER_IMP_<code>` for detailed guidance on each import diagnostic code. Use `miri explain MER_NAM_<code>` for module-path errors.
