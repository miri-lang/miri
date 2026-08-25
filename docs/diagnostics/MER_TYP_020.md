## Rule

GPU slice operations on buffers (e.g., `buffer[start..end]`) must have both bounds statically known at compile time. Open-ended ranges like `buffer[5..]` or `buffer[..end]` are not supported in GPU kernels because slice metadata cannot be determined statically.

## Before

```miri
use system.gpu
use system.collections.array

gpu fn my_kernel(data Array<f32,10>)
    let start = 2
    let slice = data[start..]

fn main()
    gpu let buf = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
    my_kernel(buf).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
    println("done")
```

## After

```miri
use system.gpu
use system.collections.array

gpu fn my_kernel(data Array<f32,10>)
    let slice = data[2..5]

fn main()
    gpu let buf = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
    my_kernel(buf).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
    println("done")
```

## Reference

[Type Checker](../reference/types.md)
