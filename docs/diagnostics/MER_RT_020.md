## Rule

A program's calls nested deeper than the stack it runs on. A function that always calls itself reaches this, and so does recursion whose depth grows with its input past what the stack holds. Releasing a long chain of objects does not: a release uses a bounded amount of stack however long the chain is, so only the program's own calls can run the stack out.

Give the recursion a base case the input reaches, or rewrite it as a loop when its depth follows the size of the data.

## Messages

- `Stack Overflow`
- `stack overflow: the program's calls are nested deeper than its stack`

## Before

```miri
use system.io

fn climb(n int) int
    return climb(n + 1) + 1

fn main()
    println(f"{climb(0)}")
```

## After

```miri
use system.io

fn climb(n int) int
    var total = 0
    for i in 0..n
        total = total + 1
    return total

fn main()
    println(f"{climb(1000)}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
