## Rule

A method that writes its collection receiver was handed a receiver something else still holds. A call to such a method first makes the collection the caller's own — copying it when another name shares it — so the method can write it in place, and the method checks that this happened before its first write through the receiver. A receiver held by more than the call means a caller skipped that step: writing it would change the value under every other name that holds it, and copying it would release a reference the method never held. So the program stops here, in every build. Ordinary Miri code cannot do this; the trap points at a compiler defect, or a test helper that hands the check a shared receiver on purpose.

## Messages

- `Receiver Arrived Shared`
- `a method that writes its receiver was handed one something else still holds`

## Before

```miri
use system.testing

fn main()
    simulate_shared_receiver()
```

## After

```miri
use system.collections.list

fn main()
    var items = List([1, 2, 3])
    let alias = items
    items.resize(5, 0)
    println(f"{items.length()} {alias.length()}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
