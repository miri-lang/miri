## Rule

Atomic operations (`atomic_add`, `atomic_sub`, `atomic_max` and the other atomic builtins) require a buffer of type `Array<Atomic<u32|i32>, N>` as their first argument. This error is raised when an atomic operation is called with a buffer of any other type. The compiler currently reports this refusal as MER_CG_008, with the message `internal compiler error: MIR lowering failed for <function>: atomic_add requires an Array<Atomic<u32|i32>, N> buffer, got <type>`; the fix is the same.

## Before

```miri
use system.collections.array
use system.gpu.atomic

fn main()
    gpu var hist = Array<f32, 4>()
    gpu forall i in 0..4
        atomic_add(hist, i, 1.0)
```

## After

```miri
use system.collections.array
use system.gpu.atomic

fn main()
    gpu var hist = Array<Atomic<u32>, 4>()
    gpu forall i in 0..4
        atomic_add(hist, i, 1 as u32)
```

## Reference

[Type Checker](../reference/types.md)
