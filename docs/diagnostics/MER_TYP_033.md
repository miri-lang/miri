## Rule

This code covers a family of member-access failures. When field names do not exist in a class, or when trying to access members on a type that does not support member access, this error is raised.

## Before

```miri
class Point
    public var x int
    public var y int

fn main()
    let p = Point(1, 2)
    let v = p.unknown_field
    println(f"{v}")
```

## After

```miri
class Point
    public var x int
    public var y int

fn main()
    let p = Point(1, 2)
    let v = p.x
    println(f"{v}")
```

## Reference

[Type Checker](../reference/types.md)
