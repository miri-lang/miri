## Rule

A type does not implement the `Accelerable` trait and cannot be GPU-resident. Only numeric primitives, booleans, and types explicitly marked as `Accelerable` may be stored in `gpu let` buffers. Custom types that contain non-accelerable fields are not allowed on the GPU.

## Messages

- `'{type}' does not implement 'Accelerable' and cannot be gpu-resident.`
- `'{type}' implements 'Accelerable' but field '{field}' has type '{fieldType}', which is not accelerable; every field of an 'Accelerable' type must itself be accelerable.`

## Before

```miri
use system.gpu

class Point
    x f32
    name String

fn main()
    gpu let points = [Point(x: 1.0, name: "A")]
```

## After

```miri
use system.gpu

struct Point
    x f32
    y f32

fn main()
    gpu let points = [Point(x: 1.0, y: 2.0)]
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
