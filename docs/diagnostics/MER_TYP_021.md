## Rule

GPU kernel functions accept buffer arguments, which must be GPU-resident (declared with `gpu let` or `gpu var`). Host-resident buffers or inline expressions cannot be passed to GPU kernels because the kernel executes on the device and can only access device memory.

## Before

```miri
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    my_kernel([1.0, 2.0, 3.0, 4.0]).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
```

## After

```miri
use system.collections.array

gpu fn my_kernel(a Array<f32,4>)
    let x = 1

fn main()
    gpu let buf = [1.0, 2.0, 3.0, 4.0]
    my_kernel(buf).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
```

## Reference

[Type Checker](../reference/types.md)
