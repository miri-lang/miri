## Rule

A method may declare type parameters of its own, `fn speak<T>(x T) String`, and each call binds them, so a body is compiled for each set of type arguments the calls use. A vtable slot holds one body, so a generic method takes no slot: every call to one runs the body the receiver's declared type reaches. A subclass redeclaring a generic method of a class it extends would therefore never run through a receiver typed as the base class, so a generic method of a class is not overridden, and a generic method does not override another.

For the same reason a class gives each generic method a body, including one a trait it implements requires without a default: no subclass could supply one. Only a trait declares a generic method without a body, and a class implementing that trait supplies it, reached through the class itself or through a type parameter bounded by the trait.

A constructor (`init`) or a drop hook (`drop`) of a class or an enum declares no type parameters of its own: building or releasing an instance runs it, and neither binds a type argument a copy could be compiled at. A type the hook needs belongs on the class or the enum.

## Messages

- `` `{method}` of `{class}` overrides the generic method `{method}` of `{base}`; every call to a generic method runs the body its receiver's type declares, so a receiver typed as `{base}` would never run it ``
- `` the generic method `{method}` of `{class}` overrides `{method}` of `{base}`; a generic method takes no vtable slot, so a receiver typed as `{base}` would never run it ``
- `` the generic method `{method}` of `{class}` has no body; only a trait may declare a generic method without one ``
- `` the abstract class `{class}` leaves the generic method `{method}` of the trait `{trait}` without a body; only a trait may declare a generic method without one ``
- `` `{method}` declares type parameters of its own, but it runs when an instance is built or released, which binds no type arguments ``

## Help

- `` a generic method cannot be overridden; move the part that varies into a non-generic method that `{class}` overrides, or declare the type parameter on the class ``
- `` a generic method cannot override another; give `{method}` another name, or declare the type parameter on the class ``
- `` give `{method}` a body: a generic method cannot be overridden, so no subclass could supply one ``
- `` give `{method}` a body in `{class}`; a generic method is compiled per call, so a subclass cannot supply it later ``
- `` declare the type parameter on the {owner} ``

## Before

```miri
use system.io

class Animal
    fn speak<T>(x T) String
        return "animal"

class Dog extends Animal
    fn speak<T>(x T) String
        return "dog"

fn main()
    let a Animal = Dog()
    println(a.speak(1))
```

## After

```miri
use system.io

class Animal
    fn speak<T>(x T) String
        return self.sound()
    fn sound() String
        return "animal"

class Dog extends Animal
    fn sound() String
        return "dog"

fn main()
    let a Animal = Dog()
    println(a.speak(1))
```

## Reference

[Type Checker](../reference/types.md)
