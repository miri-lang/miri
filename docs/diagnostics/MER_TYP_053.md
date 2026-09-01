## Rule

A vector builtin function (e.g., `vec2()`, `vec3()`) was called with an invalid number of arguments or argument types that do not match the vector's component type. Vector constructors have fixed arities and strict type requirements.

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
