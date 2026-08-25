## Rule

Atomic operations (like `atomic_add`, `atomic_exchange`, `atomic_compare_exchange`) require a buffer of type `Array<Atomic<u32|i32>, N>`. This error is raised when an atomic operation is called with a buffer of an incompatible type.

## Before

```miri
use system.gpu
use system.collections.array

gpu fn my_kernel(data Array<f32,4>)
    let old = data.atomic_add(0, 1.0)

fn main()
    gpu let buf = [1.0, 2.0, 3.0, 4.0]
    my_kernel(buf).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
```

## After

```miri
use system.gpu
use system.collections.array

gpu fn my_kernel(data Array<Atomic<u32>,4>)
    let old = data.atomic_add(0, 1)

fn main()
    gpu let buf = [1, 2, 3, 4]
    my_kernel(buf).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
```

## Reference

[Type Checker](../reference/types.md)
