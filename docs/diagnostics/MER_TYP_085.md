## Rule

An enum, struct, class or trait is declared at the top level of a file, never
inside a function or method body.

A declaration inside a body used to be accepted and then lost: the type was
gone before the next statement, so its own name was reported as an undefined
type wherever the body used it. The refusal now points at the declaration.

A type that only one function needs is declared beside that function, and
kept out of other modules with `private`.

## Messages

- `type '{name}' is declared inside a function: types are declared at the top level of a file`

## Help

- `move the declaration to the top level of the file, beside the function that uses it.`

## Before

```miri
use system.io

fn main()
    enum Door
        Open
        Shut
    let d = Door.Open
    if d == Door.Open
        println("open")
```

## After

```miri
use system.io

enum Door
    Open
    Shut

fn main()
    let d = Door.Open
    if d == Door.Open
        println("open")
```

## Reference

[Type Checker](../reference/types.md)
