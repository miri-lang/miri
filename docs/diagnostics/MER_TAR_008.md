## Rule

A GPU function (`gpu fn`) has a signature that is not compatible with GPU execution. GPU functions must not have explicit return types (they implicitly return `void`), and all parameters must be GPU-compatible types (numeric primitives, booleans, and accelerable structs only). Functions with incompatible names or signatures cannot be lowered to valid GPU shaders.

## Before

```miri
use system.gpu

gpu fn my_kernel() i32
    return 42

gpu fn another(s String)
    let x = s
```

## After

```miri
use system.gpu

gpu fn my_kernel()
    let x = 42

gpu fn another(count i32)
    let x = count
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
