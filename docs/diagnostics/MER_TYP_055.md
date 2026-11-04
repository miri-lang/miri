## Rule

A static method was accessed or defined in an invalid context. Static methods belong to the type itself rather than instances, and cannot use `self` or instance state. Attempting to use `self` in a static method or calling an instance method as static triggers this error. A static method has one body whatever its class is instantiated at, so it is also refused when its signature names one of the class's type parameters, and when it is called through an instantiation of the class (`Box<int>.seven()` rather than `Box.seven()`).

## Messages

- `'self' cannot be used in static methods`
- `Static methods cannot have a 'self' parameter`
- `Static method '{method}' cannot be called on an instance. Use '{class}.{method}()' instead.`
- `Static methods are not allowed in trait declarations`
- `Static method '{method}' is called through an instantiation of '{class}', but a static method has one body whatever the class is instantiated at`

## Before

```miri
class Counter
    var count int

    static fn reset()
        self.count = 0

fn main()
    Counter.reset()
```

## After

```miri
class Counter
    var count int

    fn reset()
        self.count = 0

fn main()
    let c = Counter(count: 3)
    c.reset()
    println(f"{c.count}")
```

## Reference

[Type Checker](../reference/types.md)
