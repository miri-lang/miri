## Rule

A generic type argument does not satisfy the constraint declared for that generic parameter. The type checker verifies that all type arguments conform to their parameter constraints at instantiation sites.

## Messages

- `Type {type} does not satisfy constraint {kind} {constraint}`
- `{type} is not one of the types {set} accepts: {members}`

## Before

```miri
trait Named
    fn name() String

fn greet<T implements Named>(x T) String
    return x.name()

fn main()
    println(greet(5))
```

## After

```miri
trait Named
    fn name() String

class Robot implements Named
    fn name() String
        return "R2"

fn greet<T implements Named>(x T) String
    return x.name()

fn main()
    println(greet(Robot()))
```

## Reference

[Type Checker](../reference/types.md)
