## Rule

Atomic operations (`atomic_add`, `atomic_sub`, `atomic_max` and the other atomic builtins) require a buffer of type `Array<Atomic<u32|i32>, N>` as their first argument. This error is raised when an atomic operation is called with a buffer of any other type.

## Messages

- `` `{builtin}` needs a buffer of `Array<Atomic<i32>, N>` or `Array<Atomic<u32>, N>`, but {buffer} is `{type}` ``

## Help

- `` declare `{name}` as `{type}` (atomics hold 32-bit integers only) ``
- `` pass a buffer declared as `{type}` (atomics hold 32-bit integers only) ``
- `` pass a buffer declared with `gpu var buf = Array<Atomic<i32>, N>()` (atomics hold 32-bit integers only) ``

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
