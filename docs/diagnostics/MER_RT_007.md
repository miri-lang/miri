## Rule

A bus error (SIGBUS) terminated the program. The processor rejected a memory access that was misaligned or fell outside a mapped region. Miri's own code generation does not emit such an access, so the realistic source is a `runtime` declaration whose signature disagrees with the intrinsic it names: the call then reads the argument at the wrong width or offset. Check that every `runtime` declaration matches the exported symbol it binds.

## Before

```miri
runtime "core" fn miri_rt_read_pair(handle int) int
```

## After

```miri
runtime "core" fn miri_rt_read_pair(handle i64, out slot i64) int
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
