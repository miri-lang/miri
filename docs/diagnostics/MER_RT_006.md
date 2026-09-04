## Rule

A segmentation fault (SIGSEGV) terminated the program. This signal indicates an attempt to access memory that was not allocated or that the program does not have permission to access. Common causes include stack overflow from unbounded recursion.

## Before

```miri
fn deep(n int) int
    return deep(n + 1)

fn main()
    let x = deep(1)
    println(f"{x}")
```

## After

```miri
fn deep(n int, limit int) int
    if n >= limit
        return 0
    return deep(n + 1, limit)

fn main()
    let x = deep(1, 1000)
    println(f"{x}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
