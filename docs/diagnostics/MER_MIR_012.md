## Rule

A struct constructor is missing a required field. All non-optional fields of a struct must be provided when constructing an instance.

## Before

```miri
struct Point
    x i32
    y i32

fn main()
    let p = Point(x: 10)
```

## After

```miri
struct Point
    x i32
    y i32

fn main()
    let p = Point(x: 10, y: 20)
```

## Reference

[MIR and Lowering](../reference/mir.md)
