## Rule

A class attempts to inherit from a type that is not a valid parent class, or violates inheritance rules such as inheriting from multiple classes or from a final type. This code covers a family of inheritance constraint violations detected during class definition.

## Before

```miri
class Animal:
  name string

class Dog < Animal < Bird:
  breed string

fn main() i32:
  0
```

## After

```miri
class Animal:
  name string

class Dog < Animal:
  breed string

fn main() i32:
  0
```

## Reference

[Type Checker](../reference/types.md)
