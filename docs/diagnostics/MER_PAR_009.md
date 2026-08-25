## Rule

The parent type in a class inheritance declaration is not a valid identifier. Inheritance syntax is `class Child extends Parent`, and `Parent` must be a valid type name. A non-identifier, literal, or other invalid token in the parent position causes this error.

## Before

```miri
class Dog extends 123
    fn bark()
        println("Woof")
```

## After

```miri
class Dog extends Animal
    fn bark()
        println("Woof")
```

## Reference

[Parser](../reference/parser.md)
