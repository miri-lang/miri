## Rule

An integer `+`, `-`, `*` or negation produced a result that does not fit the type it is computed at, and the program stopped rather than carry on with a wrapped value. The rule is the same on the processor and on the GPU. Guard the operation, compute at a wider type, or, where wrapping is what the program means — a hash, a checksum — say so with `wrapping_add`, `wrapping_sub` or `wrapping_mul` from `system.math`.

## Before

```miri
fn main()
    var total i32 = 2147483647
    total = total + 1
    println(f"{total}")
```

## After

```miri
fn main()
    var total i64 = 2147483647
    total = total + 1
    println(f"{total}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
