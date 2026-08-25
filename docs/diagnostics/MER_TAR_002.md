## Rule

An operation is not supported in GPU code. This code covers a family of GPU restrictions: functions cannot be recursive, return values cannot be optional, and static methods cannot be GPU kernels. These constraints exist because the GPU execution model does not support certain Miri features.

## Before

```miri
use system.gpu

gpu fn recursive_kernel(n i32)
    if n > 0
        recursive_kernel(n - 1)
```

## After

```miri
use system.gpu

gpu fn loop_kernel()
    var n = 5
    while n > 0
        n = n - 1
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
