## Rule

A closure holds a copy of each value it captures, made when the closure is created, and every call starts over from that copy. A write to a captured value would be gone when the call ends, so it is refused where it is written: an assignment or compound assignment to a captured binding, a write to an element or field of a captured value, a call of a method that writes its receiver on a captured value, and passing a captured value as an `out` argument. The same goes for a function declared inside another function, and for a capture reached through a nested closure.

A class instance and a value held through a trait are references: a write through one persists, so it is allowed, and it is where state that must outlive a call belongs. A closure's own parameters and locals are its own to write, including one that shadows a captured name.

## Messages

- `` a write to '{name}' would be lost when the call ends: the closure holds its own copy, made when it was created ``

## Help

- `` hold the state in a class: a closure that captures an instance shares it, so a write through it persists ``

## Before

```miri
use system.io

fn make_counter() fn() int
    var n = 0
    return fn() int:
        n += 1
        return n

fn main()
    let counter = make_counter()
    println(f"{counter()}")
```

## After

```miri
use system.io

class Counter
    var n int

fn make_counter() fn() int
    var counter = Counter(n: 0)
    return fn() int:
        counter.n += 1
        return counter.n

fn main()
    let counter = make_counter()
    println(f"{counter()} {counter()}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
