# Ownership and Resource Management

Miri uses reference counting (via the Perceus optimization) to manage object lifetimes. Resource types—classes with a drop hook—must be explicitly consumed exactly once (via drop or passing to a consuming function) to be well-formed at scope exit. Linear variables are similar: they model unique ownership and must be consumed exactly once.

## What It Rejects

- Resource variables not consumed before scope exit (warning; the drop method still runs)
- Linear variables not consumed exactly once (error)
- Use-after-move: accessing a consumed variable (error)
- Calling `drop()` on a value the scope does not own — a parameter, `self`, a field, a loop or match binding, or a captured variable (error)
- A class or trait `drop` that takes arguments or is static (error): the name belongs to the drop hook
- Discarding values that must be used (types marked `@must_use`)

## Key Concepts

- **Auto-copy types**: Types smaller than 128 bytes with only primitive fields are never moved; they are always copied
- **Managed types**: Larger types are moved at top-level scope; inside functions, they are passed by reference
- **Drop hook**: A class declares its hook as `fn drop(self)` (or `fn drop()`, since a class method's receiver is implicit), inherits one from its base class, or takes a default `drop` from a trait it implements; a struct spells it `fn drop(self)`
- **Resource types**: Classes with a drop hook must be explicitly consumed
- **Calling drop**: `value.drop()` on a local the scope declared, or on a call result, runs the hook at that point and consumes the value; the hook does not run again when the scope ends
- **Linear variables**: Universally unique; cannot be duplicated or dropped without use

## Per-Code Detail

Use `miri explain MER_OWN_<code>` for detailed guidance on each ownership diagnostic code.
