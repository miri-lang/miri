## Rule

The MIR (mid-level intermediate representation) produced during lowering failed internal validation. This occurs when the generated MIR violates an invariant that the MIR structure itself enforces. Reaching this error indicates a compiler bug in the lowering phase.

## Before

This error has no source-level reproduction; it indicates an internal MIR structure corruption.

## After

Nothing in the source program can be changed to satisfy this check, because the
failure is in the compiler rather than in the code it was given. Report it, and
include enough for someone to reproduce it:

```sh
miri check program.mi          # confirm the program itself type-checks
miri build --verify-mir program.mi   # capture the validation failure in full
```

Cut the program down to the smallest fragment that still triggers the error and
attach that fragment, the full message, and the compiler version. If the program
must keep building in the meantime, look for the construct named in the message
and express it a different way — the invariant is violated while lowering one
specific shape, so an equivalent formulation usually routes around it.

## Reference

[MIR and Lowering](../reference/mir.md)
