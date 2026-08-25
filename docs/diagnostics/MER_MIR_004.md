## Rule

The compiler could not determine the type of an expression during MIR lowering, even though the type checker already validated the code. This indicates an internal compiler inconsistency: the type checker allowed the expression but the lowering phase lacks the type information needed to generate code. Reaching this error suggests a compiler bug.

## Before

This error has no source-level reproduction; it indicates a mismatch between the type checker and MIR lowering phases.

## After

The program is not at fault: the type checker already accepted the expression, so
there is no type annotation or rewrite that makes it correct. Report it with a
reproduction:

```sh
miri check program.mi   # this succeeds, which is the point
miri build program.mi   # this fails here
```

A program that passes `check` but fails `build` at this error is exactly the
signal worth reporting — it pins the disagreement to the boundary between type
checking and lowering. Reduce it to the smallest expression that still shows the
split and attach that, with the compiler version.

## Reference

[MIR and Lowering](../reference/mir.md)
