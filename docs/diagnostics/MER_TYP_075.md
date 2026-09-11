## Rule

`<`, `<=`, `>` and `>=` are only available on a type that defines an ordering. The numeric types order by value, and a class orders its values by implementing `Comparable` — a single `compare(other Self) int` method returning a negative number when `self` sorts first, zero when neither sorts first, and a positive number when `self` sorts last.

A type that defines no ordering is refused rather than compared some other way, because the alternative comparisons are all wrong in a way nothing reports: comparing two objects by their addresses answers from allocation order, so the same two values compare differently depending on which was built first.

Equality is a separate capability and is unaffected: `==` and `!=` stay available on a type that has no ordering. Where the values are ordered by one of their members, compare that member instead.

## Messages

- `Type '{type}' has no ordering: '{operator}' requires the Comparable trait`

## Help

- `` Implement Comparable on '{type}' with `public fn compare(other Self) int` (negative sorts self first, zero ties, positive sorts self last), or compare a member that does order: `{comparison}`. ``
- `` Implement Comparable on '{type}' with `public fn compare(other Self) int` (negative sorts self first, zero ties, positive sorts self last). ``

## Before

```miri
struct Point
    x int
    y int

fn main()
    let a = Point(1, 2)
    let b = Point(3, 4)
    println(f"{a < b}")
```

## After

```miri
struct Point
    x int
    y int

fn main()
    let a = Point(1, 2)
    let b = Point(3, 4)
    println(f"{a.x < b.x}")
```

## Reference

[Type Checker](../reference/types.md)
