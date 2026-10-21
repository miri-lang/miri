## Rule

A heap block was released twice, written after it was released, or released at a size larger than the one it was allocated at. The runtime keeps small released blocks to hand out again, and marks each one it keeps; a second release of a marked block, or a mark found changed when the block is handed out, means two owners would share one block, so the program stops here instead. Ordinary Miri code cannot do any of this — reference counting releases each value once — so the trap points at a release made outside it: a runtime defect, or a test helper that releases a value on purpose.

While a check or a counter is asked for (`MIRI_LEAK_CHECK`, `MIRI_ALLOC_COUNT`, `MIRI_RC_STATS`) or `MallocScribble` is set, in a build without `--release`, the runtime also fills each released block and checks the fill when the block is handed out again, so a write through a stale reference is caught too.

## Messages

- `Heap Corruption`
- `a heap block was released twice`
- `a heap block was written after it was released`
- `a heap block was released at a size larger than it was allocated at`

## Before

```miri
use system.testing

fn main()
    simulate_double_free()
```

## After

```miri
use system.io

fn main()
    let message = "released once"
    println(message)
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
