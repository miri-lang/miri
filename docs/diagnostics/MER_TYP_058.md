## Rule

A struct definition violates structural requirements. This code covers multiple family members: duplicate field names, forbidden operations on struct types, or other constraint violations specific to struct definitions. A struct holds data only, so it declares no methods, implements no traits and has no drop hook: give a type that needs any of those a class declaration instead.

## Messages

- `Struct '{name}' cannot define method '{method}': a struct holds data only`
- `Struct '{name}' cannot implement trait '{trait}': a struct holds data only`
- `Infinite recursive type: field '{field}' of struct '{name}' contains '{type}' without indirection`

## Help

- `make '{name}' a class to give it methods, or write a function that takes the struct`

## Before

```miri
struct Point
    x int
    y int

    fn length() int
        return self.x + self.y

fn main()
    let p = Point(x: 1, y: 2)
    println(f"{p.x}")
```

## After

```miri
struct Point
    x int
    y int

fn manhattan(p Point) int
    return p.x + p.y

fn main()
    let p = Point(x: 1, y: 2)
    println(f"{manhattan(p)}")
```

## Reference

[Type Checker](../reference/types.md)
