## Rule

`miri run` and `miri build` were given a file with nothing to execute: it
declares no `main`, and it carries no top-level statement that script-mode
wrapping would demote into a synthetic one.

A file of declarations alone is a module. Checking it is meaningful and
`miri check` accepts it, which is how a library file is validated. Running it
is not: the synthetic `main` would hold nothing but its own `return 0`, so the
command used to build an executable that printed nothing and exited zero —
indistinguishable, to a caller reading the exit status, from a program that ran
and had nothing to say.

## Messages

- `this file has nothing to run: it declares no 'main' and carries no top-level statements`

## Help

- `add a 'main' function, or check the file instead with 'miri check' if it is a module.`

## Before

```miri
// lib.mi: helpers and no main, handed to `miri run`
fn double(x int) int
    return x * 2
```

## After

```miri
// app.mi: the same helper, with an entry point that uses it
fn double(x int) int
    return x * 2

fn main()
    println(f"{double(21)}")
```

A module that is meant to stay a module is checked instead: `miri check lib.mi`.

## Reference

[Build and Command Line](../reference/build.md)
