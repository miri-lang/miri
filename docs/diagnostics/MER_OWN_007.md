## Rule

A class or enum's `fn drop(self)` runs once, when the instance is released, and the instance is freed when the hook returns. While the hook runs, nothing it does to `self` can free it, so `self` is the hook's to read and to lend, never to keep: it may not be stored in a field, an element or a collection, built into a value (a struct, a class, a tuple, a variant, `Some(self)`), captured by a closure, returned, or handed to a function, a method, a trait method or a function value that keeps what it is handed. Whatever kept it would hold an instance freed as soon as the hook ends. Where the keep goes through a callee, the report follows the chain of calls to the line that keeps the instance.

Reading `self`'s fields, and lending `self` or one of its fields to a callee that only reads it, are not keeps.

## Messages

- `'{type}.drop' keeps 'self', but the instance is freed when its drop hook returns, so nothing may hold it after`

## Help

- `release what self holds explicitly; to keep a value past the hook, copy the plain data out of self's fields`

## Before

```miri
struct Record
    id int

struct Shelf
    slot Tag?

class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        let _kept = Shelf(slot: Some(self))
        println(f"dropped {self.id}")

fn main()
    let a = Tag(1)
    println(f"{a.id}")
```

## After

```miri
struct Record
    id int

class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        let kept = Record(id: self.id)
        println(f"dropped {kept.id}")

fn main()
    let a = Tag(1)
    println(f"{a.id}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
