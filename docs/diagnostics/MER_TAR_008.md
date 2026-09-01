## Rule

A GPU function (`gpu fn`) has a signature that is not compatible with GPU execution. GPU functions must not have explicit return types (they implicitly return `void`), and all parameters must be GPU-compatible types (numeric primitives, booleans, and accelerable structs only). Functions with incompatible names or signatures cannot be lowered to valid GPU shaders.

## Messages

- `GPU functions must not have an explicit return type`
- `Variable '{var}' has type '{type}' which is not GPU-compatible: only numeric primitives, booleans, and GPU types may be used inside a 'gpu fn'`
- `Discarded value of type '{type}' is not GPU-compatible: only numeric primitives, booleans, and GPU types may be produced inside a 'gpu fn'`
- `GPU function name '{name}' {msg}, so it cannot be lowered to a valid GPU shader`
- `Parameter '{param}' has type '{type}' which is not GPU-compatible: only numeric primitives, booleans, and GPU types may appear in a 'gpu fn' signature`
- `Receiver of type '{type}' is not GPU-compatible: only numeric primitives, booleans, and GPU types may appear as a member-access receiver inside a 'gpu fn'`
- `Type '{type}' is not GPU-compatible: only numeric primitives, booleans, and GPU types may cross a call boundary inside a 'gpu fn'`
- `Function '{func}' returns 'void' which is not GPU-compatible`
- `Function '{func}' returns '{type}' which is not GPU-compatible`
- `Function '{func}' has 'out' parameter '{param}' which is not GPU-compatible`
- `Function '{func}' parameter '{param}' has type '{type}' which is not GPU-compatible`
- `Function calls host-only intrinsic which is not GPU-compatible`

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
