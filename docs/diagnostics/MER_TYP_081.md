## Rule

Memory is reference counted, and reference counting never frees a cycle: an object that holds a reference to itself keeps its own count above zero and is never released. So a store that puts an object back into one of its own fields is refused. That covers storing the object itself, a closure that captures it, or a variant, constructor call or literal holding either of them into a field of that object, a field of one of its fields, or an element of a collection it holds. The check sees the direct form, where the stored value names the same binding as the target. A cycle closed through another name or another object is not caught, and it leaks.

To give an object a callback that works on it, make the object a parameter of the function and pass it at the call. To link objects to each other, keep the links in a collection owned outside them.

## Messages

- `` this store makes a reference cycle: the closure captures '{root}' and is stored in {described}, and reference counting never frees a cycle ``
- `` this store makes a reference cycle: {described} would hold '{root}' itself, and reference counting never frees a cycle ``
- `` this store makes a reference cycle: the value stored in {described} holds '{root}', and reference counting never frees a cycle ``

## Help

- `` pass the object as a parameter instead of capturing it: give the function type a parameter for '{root}' and pass it at the call ``
- `` store a different object, or keep the link outside the object, in a collection its owner holds ``

## Before

```miri
use system.io

class Ticker
    count int
    on_tick fn() int
    fn wire()
        self.on_tick = fn() int: self.count + 1

fn zero() int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick()}")
```

## After

```miri
use system.io

class Ticker
    count int
    on_tick fn(Ticker) int
    fn wire()
        self.on_tick = fn(t Ticker) int: t.count + 1

fn zero(t Ticker) int
    return 0

fn main()
    let t = Ticker(count: 1, on_tick: zero)
    t.wire()
    println(f"{t.on_tick(t)}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
