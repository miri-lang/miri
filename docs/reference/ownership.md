# Ownership and Resource Management

Miri uses reference counting (via the Perceus optimization) to manage object lifetimes. Resource types—classes with a `fn drop(self)` method—must be explicitly consumed exactly once (via drop or passing to a consuming function) to be well-formed at scope exit. Linear variables are similar: they model unique ownership and must be consumed exactly once.

## What It Rejects

- Resource variables not consumed before scope exit (warning; the drop method still runs)
- Linear variables not consumed exactly once (error)
- Use-after-move: accessing a consumed variable (error)
- Discarding values that must be used (types marked `@must_use`)

## Key Concepts

- **Auto-copy types**: Types smaller than 128 bytes with only primitive fields are never moved; they are always copied
- **Managed types**: Larger types are moved at top-level scope; inside functions, they are passed by reference
- **Resource types**: Classes with drop methods must be explicitly consumed
- **Linear variables**: Universally unique; cannot be duplicated or dropped without use

## Per-Code Detail

Use `miri explain MER_OWN_<code>` for detailed guidance on each ownership diagnostic code.
