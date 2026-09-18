## Rule

A vector was used in a way its type does not permit. Either a vector builtin (e.g. `dot`, `cross`, `normalize`) was called with the wrong number of arguments or with argument types that do not match the vector's component type — these builtins have fixed arities and strict type requirements — or the vector itself was written at a component type it cannot be laid out at.

A vector's component must have a byte width, because that width is the stride a collection spaces its elements by and the span reference counting reads an element's bytes from. The components with one are `f32`, `i32`, `u32` (the GPU-portable widths) and `f64`, `i64`, `u64`, `int`, `float` (host only — WGSL has no 64-bit vector). Any other component, `i16` or `bool` or `String` among them, is refused where it is written rather than read back as zeros.

## Messages

- `abs expects exactly one argument, but got {count}`
- `dot expects exactly two arguments, but got {count}`
- `dot expects vector with f32 elements, got {type}`
- `dot expects both vector arguments to have the same type, got {type1} and {type2}`
- `length expects exactly one argument, but got {count}`
- `length expects vector with f32 elements, got {type}`
- `normalize expects exactly one argument, but got {count}`
- `normalize expects vector with f32 elements, got {type}`
- `cross expects exactly two arguments, but got {count}`
- `cross expects Vec3 arguments, got {type}`
- `reflect expects exactly two arguments, but got {count}`
- `reflect expects vector with f32 elements, got {type}`
- `reflect expects both vector arguments to have the same type, got {type1} and {type2}`
- `mix expects exactly three arguments, but got {count}`
- `mix expects vector with f32 elements, got {type}`
- `mix expects both vector arguments to have the same type, got {type1} and {type2}`
- `Vector component type '{type}' is not supported`

## Before

```miri
use system.gpu.vector

fn main()
    let v1 = Vec2<f32>(1.0, 2.0)
    let v2 = Vec2<f32>(3.0, 4.0)
    let result = dot(v1, v2, 5.0)
    println("ok")
```

## After

```miri
use system.gpu.vector

fn main()
    let v1 = Vec2<f32>(1.0, 2.0)
    let v2 = Vec2<f32>(3.0, 4.0)
    let result = dot(v1, v2)
    println("ok")
```

## Reference

[Type Checker](../reference/types.md)
