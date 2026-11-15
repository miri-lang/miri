## Rule

A container copies, orders, matches and hashes class elements through their own `clone`, `compare`, `equals` and `hash`. The container calls these itself, with elements it only lends: it still holds every element after the call, so the method cannot become the owner of one. A method a container calls that keeps a resource it is handed (stores it, returns it, hands it to a method that keeps it) is refused where it is declared, its receiver included. The methods are recognised by the class that reaches them: `clone` on a class implementing `Cloneable`, `compare` on one implementing `Comparable`, an `equals` taking one value of the class's own type and returning `bool`, and `hash` on a class that declares `equals`. A method of one of these names that no container calls is an ordinary method, and it owns what it keeps.

## Messages

- `'{type}.{method}' keeps '{param}', but a container calls '{method}' on elements it only lends, so it cannot keep one`

## Help

- `make '{method}' only read '{param}', and keep the value in a method the program calls by name`

## Before

```miri
class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        let _kept = Shelf(slot: Some(other))
        return true

struct Shelf
    slot Tag?

fn main()
    let a = Tag(1)
    println(f"{a.id}")
```

## After

```miri
class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"dropped {self.id}")
    public fn equals(other Tag) bool
        return self.id == other.id
    public fn shelve(other Tag) bool
        let _kept = Shelf(slot: Some(other))
        return true

struct Shelf
    slot Tag?

fn main()
    let a = Tag(1)
    println(f"{a.id}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
