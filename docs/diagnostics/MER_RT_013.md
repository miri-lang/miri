## Rule

A GPU kernel launch was refused, so the program stopped before the kernel ran. The runtime prints the reason first: a grid wider than the device allows (a dimension past `u32::MAX` counts as too wide rather than wrapping to a smaller grid), a captured value or buffer element that does not fit its 32-bit device lane, or a kernel the device would not compile. Size the launch to the data, and keep values that travel through a narrowed lane inside its range.

## Before

```miri
use system.gpu
use system.collections.array

gpu fn fill(c out Array<f32, 4>)
    c[0] = 1.0

fn main()
    gpu var c = Array<f32, 4>()
    fill(c).launch(Dim3(100000, 1, 1), Dim3(1, 1, 1))
```

## After

```miri
use system.gpu
use system.collections.array

gpu fn fill(c out Array<f32, 4>)
    c[0] = 1.0

fn main()
    gpu var c = Array<f32, 4>()
    fill(c).launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
