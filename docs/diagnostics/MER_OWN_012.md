## Rule

An assignment is an expression whose value is its target's: `let y = (x = 3)` binds 3. When the target's type holds a resource — a class or enum with a `drop` hook, or a tuple or optional holding one — the target keeps what the assignment stores, and the value the assignment gives back would be a second holder of it, whose hook would run when that holder lets go rather than when the target does. A resource has one holder, so reading the value of an assignment that stores one is refused: as an initializer, an operand, an argument, a returned value, or the right side of a chained assignment such as `p = q = r`. The same assignment written as a statement of its own is accepted, and so is reading the value of an assignment that stores a plain value.

## Messages

- `` the value of the assignment to `{target}` is read, and `{target}` keeps the resource it stores: a resource has one holder, so an assignment that stores one gives back no value ``

## Help

- `` write the assignment to `{target}` as a statement of its own, then read `{target}` ``

## Before

```miri
use system.collections.list

class Token
    var id int
    fn drop(self)
        println(f"released {self.id}")

fn show(token Token)
    println(f"{token.id}")

fn main()
    var tokens = List([Token(id: 9)])
    show(tokens[0] = Token(id: 1))
```

## After

```miri
use system.collections.list

class Token
    var id int
    fn drop(self)
        println(f"released {self.id}")

fn show(token Token)
    println(f"{token.id}")

fn main()
    var tokens = List([Token(id: 9)])
    tokens[0] = Token(id: 1)
    show(tokens[0])
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
