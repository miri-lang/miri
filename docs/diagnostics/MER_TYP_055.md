## Rule

A static method was accessed or defined in an invalid context. Static methods belong to the type itself rather than instances, and cannot use `self` or instance state. Attempting to use `self` in a static method or calling an instance method as static triggers this error.

## Messages

- `'self' cannot be used in static methods`
- `Static methods cannot have a 'self' parameter`
- `Static method '{method}' cannot be called on an instance. Use '{class}.{method}()' instead.`
- `Static methods are not allowed in trait declarations`

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
