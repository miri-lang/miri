## Rule

A `shared` variable was declared where it is not valid. `shared` declares workgroup memory, which exists only while a GPU kernel runs: it may appear only inside a `gpu fn`, must be an array with an explicit type, and cannot have an initializer, because every thread of the workgroup sees the same storage.

## Messages

- `Shared variables can only be declared inside 'gpu' functions`
- `Shared variable '{name}' must be an array, got {type}`
- `Shared variable '{name}' must have an explicit type`
- `Shared variable '{name}' cannot have an initializer`

## Before

```miri
use system.collections.array

fn main()
    shared tile Array<f32, 8>
    println("done")
```

## After

```miri
use system.collections.array

gpu fn smooth_tile(data out Array<f32, 8>)
    shared tile Array<f32, 8>
    let i = kernel.thread_idx.x
    tile[i] = data[i]
    kernel.barrier()
    data[i] = tile[i]

fn main()
    gpu var data = Array<f32, 8>()
    smooth_tile(data).launch(Dim3(1, 1, 1), Dim3(8, 1, 1))
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
