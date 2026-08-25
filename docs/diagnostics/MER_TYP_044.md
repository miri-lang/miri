## Rule

A type (class, struct, enum, or trait) was defined with the same name as a previously defined type in the current scope. Each type name must be unique within its defining module to avoid ambiguity.

## Before


```miri
class Point
    var x int

class Point
    var y int

fn main()
    println("done")
```

## After


```miri
class Point
    var x int

class Label
    var y int

fn main()
    println("done")
```

## Reference

[Type Checker](../reference/types.md)
