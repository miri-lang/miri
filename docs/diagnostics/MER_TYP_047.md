## Rule

A class attempts to inherit from a type that is not a valid parent class, or violates inheritance rules such as inheriting from multiple classes or from a final type. This code covers a family of inheritance constraint violations detected during class definition.

## Messages

- `Method '{method}' has incompatible return type: expected {expected}, got {got}`
- `Method '{method}' has incompatible parameter count: parent has {parent} parameters, child has {child}`
- `Base class '{name}' is not defined`
- `Circular inheritance detected: class '{name}' eventually extends itself`

## Before

```miri
class Shape
    fn area() float
        return 0.0

class Square extends Shape
    var side float

    fn area() int
        return 1

fn main()
    let s = Square(side: 2.0)
    println(f"{s.area()}")
```

## After

```miri
class Shape
    fn area() float
        return 0.0

class Square extends Shape
    var side float

    fn area() float
        return self.side * self.side

fn main()
    let s = Square(side: 2.0)
    println(f"{s.area()}")
```

## Reference

[Type Checker](../reference/types.md)
