## Rule

A segmentation fault (SIGSEGV) terminated the program. This signal indicates an attempt to access memory that was not allocated or that the program does not have permission to access. Safe Miri does not reach this on its own: it comes from code outside the language's guarantees, such as a `runtime` function. A stack overflow is reported separately, as MER_RT_020.

## Messages

- `terminated by signal {number} ({name})`

## Before

```miri
runtime "core" fn miri_rt_test_simulate_wild_read()

fn main()
    miri_rt_test_simulate_wild_read()
```

## After

```miri
use system.io

fn main()
    println("no wild read")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
