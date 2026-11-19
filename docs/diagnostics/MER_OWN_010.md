## Rule

A collection operation that copies its elements, `clone`, `filter`, `resize`, `push_copy` and their kin, copies each element with the element's own `clone` when its type implements `Cloneable`, so every copy is a separate value with its own `drop` hook. An element of a resource type that does not implement `Cloneable` cannot be copied: two holders of one resource would run its hook once, where the later of them ends. Such a call is refused where the program makes it, with a note naming the line of the operation that copies. A resource that has no `clone` the collection can call, a struct or an enum with a hook or a collection of resources, is refused even when it implements `Cloneable`. An element of a plain type that does not implement `Cloneable` is shared, as copy-on-write keeps it safe.

## Messages

- `` `{method}` copies elements of `{element}`, a resource with no `clone` to copy it with ``
- `` this call copies `{element}`, a resource with no `clone` to copy it with ``

## Help

- `` implement `{copy_trait}` on `{element}` so each copy is its own value, or hand each value over instead of copying it ``

## Before

```miri
use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn main()
    let conns = List([Conn(1), Conn(2)])
    let copy = conns.clone()
    println(f"{copy.length()}")
```

## After

```miri
use system.collections.list
use system.memory

class Conn implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn clone() Conn
        return Conn(self.id)
    fn drop(self)
        println(f"closed {self.id}")

fn main()
    let conns = List([Conn(1), Conn(2)])
    let copy = conns.clone()
    println(f"{copy.length()}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
