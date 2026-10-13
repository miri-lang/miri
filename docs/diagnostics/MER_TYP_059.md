## Rule

A name used where an enum is required does not name an enum, or an enum definition violates its structural requirements. A variant pattern or a `Type.Variant` path must name an enum the program declares or imports; matching a struct or class against `Type.Variant` causes this error. Match on the value's fields, or declare the type as an enum.

## Messages

- `'{name}' is not an Enum`
- `Invalid enum name`

## Before

```miri
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    match p
        Point.Origin: println("origin")
        _: println("elsewhere")
```

## After

```miri
struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    if p.x == 0 and p.y == 0
        println("origin")
    else
        println("elsewhere")
```

## Reference

[Type Checker](../reference/types.md)
