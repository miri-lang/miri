## Rule

A `continue` statement was used outside a loop, or inside a `gpu forall` body, where the dispatch is not an iterative loop. `continue` skips to the next iteration of the loop around it, so it has meaning only inside a `while` or `for` body. A `break` in the same position is reported as MER_TAR_005.

## Messages

- `Continue statement outside of loop`
- `'continue' is not supported inside a 'gpu forall' body: the GPU dispatch is not an iterative loop, so loop-control statements have no meaning at the kernel level`

## Before

```miri
fn main()
    let n = 3
    if n > 2
        continue
    println(f"{n}")
```

## After

```miri
fn main()
    var i = 0
    while i < 5
        i = i + 1
        if i == 3
            continue
        println(f"{i}")
```

## Reference

[Type Checker](../reference/types.md)
