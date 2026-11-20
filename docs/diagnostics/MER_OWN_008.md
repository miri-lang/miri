## Rule

A resource read out of a place the code does not own whole is borrowed: it may be read and lent to a call that only reads it, but it may not be handed to a position that keeps what it is handed — a function keeping its argument, a method keeping its receiver, a call through a function value, or a trait or overridable method whose slot owns it. Kept there, the value would have two holders, the place and the keeper, and its `drop` hook would run where the later of them ends rather than where the keeper ends.

The places are an `out` parameter and the receiver of a method that replaces it, both of which the caller gets back on every exit; an optional field or element; an element of a list; the element a `for` loop binds; what a method hands back out of a place of its receiver, `l.element_at(i)` or a getter returning `self.items[i]`, while the receiver still holds it; the payload a `match` binds out of one of these; and a local bound to any of them. Ownership leaves a place only through an operation that empties it, which hands the value over owned:

- an `out` parameter: `replace(c, <new value>)` gives the caller a new value and returns the old one; a receiver the method restores: `return replace(self, <new value>)`;
- an optional: `h.slot.take()` leaves `None` behind, so `match h.slot.take()` binds a payload the arm owns;
- a list element: `match l.remove_at(i)`; a loop over a list: a `while` loop removing the elements one at a time;
- what a getter hands back: the receiver's emptying method, when its type has one, or a copy, `pool.get(0).clone()`, when the element's type implements `Cloneable`.

Handing an `out` parameter to a keeper and then assigning it a new value, `keep(c)` followed by `c = Conn(2)`, is refused as well: the check does not follow the assignment that would give the caller its place back, so `keep(replace(c, Conn(2)))` is the spelling that hands the old value over.

A generic body is refused at each instantiation reading the place as a resource, and runs at one that does not. Each repair names what the program's declarations provide: the help names the optional's emptying method and the replacing function with the module to load it from, and leaves a repair out when nothing provides it. A method is an emptying method when the value it returns is taken out of `self`: by `replace`, by a runtime operation that removes the element the method read, or by another emptying method called on a place of `self`. The compiler reads this from the method's body, so a collection of the program's own whose `pull(i int) T?` returns `self.items.remove_at(i)` is named in the repair exactly as `remove_at` is.

A `match` on a value the scope owns — a local, a parameter, a call's result — hands its payload over instead: keeping the payload consumes the value matched, which ends there, and a later use of it is refused (`MER_OWN_003`). A plain, non-optional resource field is still handed to a keeper as a copy, its holder keeping its own.

## Messages

- `'{value}' is borrowed from {place}, so {keeper} cannot keep it`

## Help

- `` take the value out, leaving {absent} behind, and hand that on: `{path}.{method}()` ``
- `` take the value out before matching, leaving {absent} behind: `match {path}.{method}()` ``
- `` remove the element and hand that on: `match {list}.{method}({index})` gives its arm the element, owned ``
- `` remove the elements instead of walking them: in a `while` loop, `match {list}.{method}(0)`, hand on the element of `{present}`, and `break` on `{absent}` ``
- `` give the caller a new value and hand on the old one: `{replace}({name}, <new value>)`{use} ``
- `` hand back the old value and leave a new one in self: `return {replace}(self, <new value>)`{use} ``
- `` take it out instead, with `{receiver}.{method}`, which hands it over owned, or keep a copy, `{call}.clone()`, when its type implements `{copy_trait}` ``
- `` keep a copy, `{call}.clone()`, when its type implements `{copy_trait}`: '{receiver}' keeps holding the value the call hands back ``

## Before

```miri
use system.collections.list

class Conn
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn keep(c Conn) Conn?
    return Some(c)

fn main()
    let conns = List([Conn(1), Conn(2)])
    println(f"{keep(conns[0]).is_some()}")
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

fn keep(c Conn) Conn?
    return Some(c)

fn main()
    let conns = List([Conn(1), Conn(2)])
    match conns.remove_at(0)
        Some(c):
            println(f"{keep(c).is_some()}")
        None: println("empty")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
