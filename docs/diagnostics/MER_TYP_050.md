## Rule

A `break` or `continue` statement was used outside a loop. `break` leaves the loop around it and `continue` skips to its next iteration, so each has meaning only inside a `while`, `for` or other loop body. Inside a `gpu forall` body, where the dispatch is not an iterative loop, both are reported as the GPU restriction MER_TAR_005 instead.

## Messages

- `Break statement outside of loop`
- `Continue statement outside of loop`

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
