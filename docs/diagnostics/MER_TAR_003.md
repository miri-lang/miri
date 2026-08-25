## Rule

A division or modulo operand is an integer literal outside the 32-bit signed range representable on the GPU. On the GPU, `int` is 32-bit, so large literal constants are silently truncated by the WGSL backend, causing division by a different value than the source spells. Cast the operand to `i32` or use a value that fits.

## Before

```miri
use system.gpu

gpu fn my_kernel()
    gpu let a = [10, 20, 30]
    let result = a[0] / 9223372036854775807
```

## After

```miri
use system.gpu

gpu fn my_kernel()
    gpu let a = [10, 20, 30]
    let result = a[0] / i32(2)
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
