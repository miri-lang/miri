## Rule

The program's `main` takes no parameters and returns an integer or nothing.

`main` is called once, with no arguments, when the program starts, and the
value it returns becomes the process's exit status. A parameter would be read
from an argument nothing passed, and a result that is not an integer has no
exit status to give. Both used to compile: a `main` taking a list of strings
crashed reading it, and one returning a string exited with part of its address.

A program reads its command-line arguments while it runs, through `Args` in
`system.os`. Any integer width is an exit status; the operating system keeps
its low byte.

Only the program's own top-level `main` is its entry point. A method named
`main`, a function named `main` declared inside another function, or one in an
imported module is an ordinary function.

## Messages

- `'main' takes no parameters, but declares '{name}': nothing passes the program an argument for it`
- `'main' returns the program's exit status, so it returns an integer or nothing, not '{type}'`

## Help

- `remove the parameters; a program reads its command-line arguments while it runs, not as parameters of 'main'.`
- `declare 'main' with no return type, or return an integer status such as 'int'.`

## Before

```miri
use system.io

fn main(args [String])
    println(f"{args.length()}")
```

## After

```miri
use system.io
use system.os

fn main()
    let args = Args()
    println(f"{args.length()}")
```

## Reference

[Type Checker](../reference/types.md)
