## Rule

A type cannot be GPU-resident. Only device-storable numbers, vectors, structs whose every field is accelerable, and classes marked `Accelerable` may be stored in `gpu let` buffers. A struct needs no marker: it is data, so whether it can live on the GPU is decided by its fields. A type that contains a non-accelerable field is not allowed on the GPU.

## Messages

- `'{type}' does not implement 'Accelerable' and cannot be gpu-resident.`
- `'{type}' implements 'Accelerable' but field '{field}' has type '{fieldType}', which is not accelerable; every field of an 'Accelerable' type must itself be accelerable.`
- `'{type}' is a struct whose field '{field}' has type '{fieldType}', which is not accelerable; a struct is gpu-resident when every field is accelerable.`

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
