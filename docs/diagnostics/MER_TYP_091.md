## Rule

An enum method may assign its whole receiver, `self = ...`: the call takes the caller's place and restores it with what the method leaves there, the way an `out` argument is. A class instance keeps its state in its fields, so a class method, an extended class's method and a trait's default method assign the fields instead; `self = ...` there is refused.

An enum's `init` and `drop` run as an instance is built or released, and its `clone`, `compare`, `equals` and `hash` are called by a container on elements it only lends. None of them has a place to restore a replaced receiver to, so assigning `self` in one of them, or calling on `self` a method that does, is refused as well.

## Messages

- `` '{owner}.{method}' assigns its whole receiver: an instance of '{owner}' keeps its state in its fields, so `self = ...` is refused outside an enum ``
- `` '{owner}.{method}' replaces its receiver, but {reason}, so no caller's place gets the new value back ``

## Help

- `` assign the fields instead (self.n = 0) ``
- `` return the new value instead, or replace the receiver in a method of its own ``

## Before

```miri
use system.io

class Counter
    var n int

    fn restart()
        self = Counter(n: 0)

fn main()
    let counter = Counter(n: 3)
    counter.restart()
    println(f"{counter.n}")
```

## After

```miri
use system.io

class Counter
    var n int

    fn restart()
        self.n = 0

fn main()
    let counter = Counter(n: 3)
    counter.restart()
    println(f"{counter.n}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
