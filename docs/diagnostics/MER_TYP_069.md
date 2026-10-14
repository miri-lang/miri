## Rule

A file that declares its own `main` is not a script, so a top-level statement
beside it has nowhere to run.

Miri wraps a script — a file of top-level statements with no `main` — in a
synthetic `main` so those statements execute in source order. A file that
declares `main` skips that wrapping, because the author has already said where
the program starts. A top-level statement left in such a file is therefore
never lowered and never runs; before this check it was discarded in silence,
and the program compiled, ran, and produced output missing whatever the
statement would have printed.

A top-level `const` is not this diagnostic. A `const` is a compile-time value
with no execution order, so it stays at the top level in both shapes and
remains visible to every function in the file. Only statements that would have
executed — expressions, `let`/`var` bindings, `if`, `while`, `for`, `forall`,
blocks and jumps — are rejected.

`miri fix` moves the statements for you through the `move-into-main` repair: every statement beside `main` goes to the start of its body, in the order written, indented to match. It is offered only when the move cannot change what a name refers to — when no other function reads a name those statements bind, and `main` neither takes nor declares one of the same name. Otherwise the choice between moving the statement and dropping `main` is yours.

## Messages

- `top-level statement will never run: this file declares 'main', so nothing executes it`

## Help

- `move the statement into 'main', or remove the 'main' declaration so the file runs as a script.`

## Before

```miri
println("never runs")

fn main()
    println("in main")
```

## After

```miri
fn main()
    println("never runs")
    println("in main")
```

## Reference

[Type Checker](../reference/types.md)
