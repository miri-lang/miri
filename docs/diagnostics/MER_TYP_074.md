## Rule

A statement follows one that ends its block. `return`, `break` and `continue` all leave, so nothing written after them in the same block runs.

This is reported wherever the statement is written, including a statement that follows a `return` inside a loop body or a branch. Move the statement above the one that leaves, or remove it.

## Messages

- `Unreachable statement: the '{keyword}' above leaves this block, so nothing after it runs`

## Before

```miri
fn announce()
    println("first")
    return
    println("never")

fn main()
    announce()
```

## After

```miri
fn announce()
    println("first")
    println("second")

fn main()
    announce()
```

## Reference

[Type Checker](../reference/types.md)
