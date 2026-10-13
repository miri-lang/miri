## Rule

A class definition violates structural requirements. This code covers multiple family members: duplicate method names, invalid member declarations, forbidden inheritance patterns, or other rule violations specific to class syntax and semantics.

## Messages

- `Non-abstract class '{class}' cannot have abstract method '{method}'`
- `'{name}' is not a class`
- `Only field and method declarations are allowed in class body`

## Before

```miri
class Shape
    abstract fn area() float

fn main()
    println("shapes")
```

## After

```miri
abstract class Shape
    abstract fn area() float

fn main()
    println("shapes")
```

## Reference

[Type Checker](../reference/types.md)
