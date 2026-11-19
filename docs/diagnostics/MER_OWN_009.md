## Rule

A method whose body calls its function-typed parameter `f` with a value it does not own whole lends `f` the argument at that position. Such a value is an element of the receiver, a `for` binding, a value a getter hands back out of a place, or a local bound to one of these. The method still holds the value once the call returns. The function a caller passes for `f` may therefore keep none of its parameters of a resource type at a lent position. It may not return one, store one, capture one or hand one to a function that keeps it, because the value would then have two holders and its `drop` hook would run where the later of them ends. A lambda literal and a named function are judged by their own bodies. A function value whose body cannot be seen is refused where it would be lent a resource. A position the method hands only values it owns, such as the accumulator of a `reduce`, is not lent, and a parameter whose type is no resource at the call is never refused.

The standard library's `filter`, `map`, `any`, `all`, `count_where`, `flat_map` and `sorted_by` lend each element to their function parameter, and `reduce` lends the element but not the accumulator.

## Messages

- `` `{method}` lends the function passed as `{parameter}` its argument {position}, so that function cannot keep it ``

## Help

- `pass a function that only reads what it is handed, and copy out the plain data it needs to keep`

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
    let same = conns.map(fn(c Conn) Conn: c)
    println(f"{same.length()}")
```

## After

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
    let ids = conns.map(fn(c Conn) Conn: Conn(c.id + 10))
    println(f"{ids.length()}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
