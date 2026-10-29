## Rule

A `for` loop binds its variable at one type, so it can only walk a tuple whose elements all have that one type. A tuple whose elements differ in type is refused, rather than read at its first element's type. Two types count as one only when they are the same type: an `i32` and an `int` are different types laid out at different widths, even where each value would fit the other.

Read the elements of a mixed tuple by position instead, or convert them with `as` so they share a type.

## Messages

- `a for loop cannot walk the tuple '{tuple}': its elements are not all one type`

## Help

- `` a loop variable has one type; read each element by position (`t.0`, `t.1`), or convert the elements with `as` so every one has the same type ``

## Before

```miri
use system.io

fn main()
    let pair = (1, "one")
    for part in pair
        println(f"{part}")
```

## After

```miri
use system.io

fn main()
    let pair = (1, "one")
    println(f"{pair.0} {pair.1}")
```

## Reference

[Type Checker](../reference/types.md)
