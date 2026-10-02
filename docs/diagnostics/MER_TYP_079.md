## Rule

A Set places its elements, and a Map its keys, by their hash, and then matches them with `==`. Two values `==` calls equal must therefore hash alike. A type whose equality is derived — a scalar, a string, an optional, a struct or enum compared part by part, a class compared by identity — derives its hash from the same parts. A type that writes its own `equals` decides alone which of its values are equal, so it must write a `hash` consistent with it: implement `Hashable`, returning the same value for any two values `equals` calls equal. Until it does, it is refused as a Set element or Map key, and wherever its `hash()` is called. `hash_combine` folds the hashes of the parts `equals` compares into one.

## Messages

- `` '{type}' defines its own 'equals' but no 'hash' consistent with it ``

## Help

- `` implement 'Hashable' on '{type}' with a 'hash()' that returns the same value for any two values its 'equals' calls equal ``

## Before

```miri
use system.io
use system.collections.set

class Point implements Equatable
    x int
    y int
    fn equals(other Point) bool
        return self.x == other.x and self.y == other.y

fn main()
    let points = Set<Point>()
    points.add(Point(x: 1, y: 2))
    println(f"{points.length()}")
```

## After

```miri
use system.io
use system.collections.set

class Point implements Equatable, Hashable
    x int
    y int
    fn equals(other Point) bool
        return self.x == other.x and self.y == other.y
    fn hash() int
        return hash_combine(self.x.hash(), self.y.hash())

fn main()
    let points = Set<Point>()
    points.add(Point(x: 1, y: 2))
    println(f"{points.length()}")
```

## Reference

[Type Checker](../reference/types.md)
