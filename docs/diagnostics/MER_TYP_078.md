## Rule

A method is called on the value it is read from: `k.a()` runs `a` with `k` as its receiver. Read without a call — `let g = k.a` — the method has no receiver bound to it, so it names no function a caller could run, and the program is refused. To hand a method on as a value, wrap the call in a lambda, which captures the receiver: `let g = fn() int: k.a()`. A field holding a function value is not a method and is read as a value as usual.

## Messages

- `` the method `{method}` is read as a value, but a method can only be called ``

## Help

- `` call it, or wrap the call in a lambda to hand it on as a value: `fn() int: obj.method()` ``

## Before

```miri
use system.io

class K
    n int
    fn a() int
        return self.n

fn main()
    let k = K(n: 4)
    let g = k.a
    println(f"{g()}")
```

## After

```miri
use system.io

class K
    n int
    fn a() int
        return self.n

fn main()
    let k = K(n: 4)
    let g = fn() int: k.a()
    println(f"{g()}")
```

## Reference

[Type Checker](../reference/types.md)
