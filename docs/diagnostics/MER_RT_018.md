## Rule

A heap block was handed to the runtime after it was released. The heap guard
(`MIRI_HEAP_GUARD=1`) records every block's allocation and release, and every
runtime function that receives a managed value asks it first; a block already
released stops the program here, before anything reads the memory another
allocation may now own. The report names where the block was allocated and
where it was released.

Ordinary Miri code cannot release a value it still uses: the compiler counts
every reference. The trap points at a compiler defect in that counting, or a
test helper that hands the runtime a released block on purpose. Without the
guard the same fault goes unreported and reads freed memory.

## Messages

- `Use After Free`
- `MIRI_HEAP_GUARD: use-after-free detected: intrinsic at {site} touched pointer allocated at {site}, freed at {site}`

## Before

```miri
use system.testing

fn main()
    simulate_use_after_free()
```

## After

```miri
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    println(f"{items.length()}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
