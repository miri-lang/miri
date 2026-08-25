## Rule

A value was used inside a string interpolation expression (e.g., `"value: {x}"`) but its type is not valid for string interpolation. Only types with well-defined string representations can be interpolated; custom types without `to_string` implementations or certain special types are forbidden.

## Before

```miri
class Point
    public var x int
    public var y int

fn main()
    let p = Point(1, 2)
    let s = f"point: {p}"
    println(f"{s}")
```

## After

```miri
class Point
    public var x int
    public var y int

fn main()
    let x = 42
    let s = f"value: {x}"
    println(f"{s}")
```

## Reference

[Type Checker](../reference/types.md)
