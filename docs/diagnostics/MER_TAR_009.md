## Rule

An array element in a GPU-resident buffer has a value that exceeds the 32-bit signed range (`-2147483648` to `2147483647`). On the GPU, array elements are stored as 32-bit integers, so large literal values are silently truncated. To use wider integer types, explicitly specify the element type in the array constructor.

## Before

```miri
use system.gpu

fn main()
    gpu let data = [9223372036854775807, 100, 200]
```

## After

```miri
use system.gpu

fn main()
    gpu let data = [i32(2000000000), i32(100), i32(200)]
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
