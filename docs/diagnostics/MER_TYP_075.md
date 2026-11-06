## Rule

`<`, `<=`, `>` and `>=` are only available on a type that defines an ordering. The numeric types order by value, and a class orders its values by implementing `Comparable` — a single `compare(other Self) int` method returning a negative number when `self` sorts first, zero when neither sorts first, and a positive number when `self` sorts last.

A type that defines no ordering is refused rather than compared some other way, because the alternative comparisons are all wrong in a way nothing reports: comparing two objects by their addresses answers from allocation order, so the same two values compare differently depending on which was built first.

`x in r` on a range stands for `r.start <= x` and `x < r.end`, so it asks the same of the range's element type and of `x`: a range of a type without an ordering, or a value without one, is refused under this code too.

Equality is a separate capability and is unaffected: `==` and `!=` stay available on a type that has no ordering. Where the values are ordered by one of their members, compare that member instead.

## Messages

- `Type '{type}' has no ordering: '{operator}' requires the Comparable trait`
- `Type '{type}' has no ordering: testing it against a range's bounds requires the {trait} trait`

## Help

- `` Implement Comparable on '{type}' with `public fn compare(other Self) int` (negative sorts self first, zero ties, positive sorts self last), or compare a member that does order: `{comparison}`. ``
- `` Implement Comparable on '{type}' with `public fn compare(other Self) int` (negative sorts self first, zero ties, positive sorts self last). ``
- `` `x in r` compares `r.start <= x` and `x < r.end`, and '{type}' has no ordering: unwrap the optional before testing it. ``
- `` `x in r` compares `r.start <= x` and `x < r.end`, and '{type}' has no ordering: test one of its elements instead. ``
- `` `x in r` compares `r.start <= x` and `x < r.end`, and '{type}' has no ordering: test a member that orders, such as `{member}`. ``
- `` `x in r` compares `r.start <= x` and `x < r.end`, and '{type}' has no ordering: implement Comparable on '{type}' with `public fn compare(other Self) int`, or test a member that orders, such as `{member}`. ``
- `` `x in r` compares `r.start <= x` and `x < r.end`, and '{type}' has no ordering: implement Comparable on '{type}' with `public fn compare(other Self) int`. ``

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
