## Rule

GPU kernel functions accept buffer arguments, which must be GPU-resident (declared with `gpu let` or `gpu var`). Host-resident buffers or inline expressions cannot be passed to GPU kernels because the kernel executes on the device and can only access device memory.

## Messages

- `` a gpu fn reads its buffers from device memory, but `{name}` is host-resident ``
- `` a gpu fn reads its buffers from device memory, but an array built in the call is host-resident ``

## Help

- `` declare it on the device: `gpu let {name} = ...` (or `gpu var` to write it) ``
- `` bind it on the device first, e.g. `gpu let buf = [...]`, and pass `buf` ``

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
