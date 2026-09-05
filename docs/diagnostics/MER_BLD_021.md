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

## Before

```sh
# lib.mi declares helpers and no main
miri run lib.mi
# error[MER_BLD_021]: Nothing to Run
```

## After

```sh
# Check the module instead — that is the question a module can answer.
miri check lib.mi

# Or give the file an entry point and run it.
miri run app.mi
```

## Reference

[Build and Command Line](../reference/build.md)
