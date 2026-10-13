## Rule

The parent type in a class inheritance declaration is not a plain type name. Inheritance syntax is `class Child extends Parent`, where `Parent` names a class in scope, with type arguments if it is generic. A qualified path such as `animals::Animal` in the parent position causes this error: import the class and name it directly.

## Messages

- `Invalid Inheritance Identifier`

## Help

- `Parent type in inheritance must be a valid identifier.`

## Before

```miri
class Animal
    fn speak()
        println("...")

class Dog extends animals::Animal
    fn bark()
        println("Woof")
```

## After

```miri
class Animal
    fn speak()
        println("...")

class Dog extends Animal
    fn bark()
        println("Woof")
```

## Reference

[Parser](../reference/parser.md)
