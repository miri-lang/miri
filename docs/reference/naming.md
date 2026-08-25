# Naming and Identifiers

Naming validation checks that identifiers in the program refer to types and values that exist and are accessible. It also enforces backward-compatibility warnings for spelling changes in the language.

## What It Rejects

- Deprecated identifier spellings (e.g., old names for GPU kernel context)
- References to types or modules that do not exist
- Syntactically invalid import paths (paths with slashes, parent-directory references, or invalid characters)

## Common Errors

Naming errors often indicate a typo in an identifier or an attempt to use a feature that is no longer spelled that way. Deprecation warnings point to the old spelling and the new one to use instead.

## Per-Code Detail

Use `miri explain MER_NAM_<code>` for detailed guidance on each naming diagnostic code.
