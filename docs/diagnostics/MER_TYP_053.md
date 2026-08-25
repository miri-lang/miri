## Rule

A vector builtin function (e.g., `vec2()`, `vec3()`) was called with an invalid number of arguments or argument types that do not match the vector's component type. Vector constructors have fixed arities and strict type requirements.

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
