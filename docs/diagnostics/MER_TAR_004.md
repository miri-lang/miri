## Rule

A `kernel.barrier()` call sits under control flow that differs between the threads of a workgroup. Every thread in a workgroup must reach the same barrier; a barrier inside a branch or loop guarded by a per-thread index (`kernel.thread_idx`, `kernel.global_idx`) is reached by some threads and not others, which deadlocks the workgroup. Move the barrier to control flow every thread takes.

## Messages

- `'kernel.barrier()' under thread-divergent control flow deadlocks the workgroup: some threads reach the barrier and others do not. Move the barrier to uniform control flow (reached by every thread in the workgroup).`

## Before

```miri
use system.collections.array

gpu fn blur(data out Array<f32, 64>)
    let i = kernel.thread_idx.x
    if i < 32
        kernel.barrier()
    data[i] = 1.0

fn main()
    gpu var data = Array<f32, 64>()
    blur(data).launch(Dim3(1, 1, 1), Dim3(64, 1, 1))
```

## After

```miri
use system.collections.array

gpu fn blur(data out Array<f32, 64>)
    let i = kernel.thread_idx.x
    kernel.barrier()
    data[i] = 1.0

fn main()
    gpu var data = Array<f32, 64>()
    blur(data).launch(Dim3(1, 1, 1), Dim3(64, 1, 1))
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
