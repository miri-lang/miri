## Rule

`for x in a..b` steps through a range: it starts at `a`, and while the value has not passed `b` it runs the body and takes the next value. Only a type with a next value can be stepped through, and that is what the `Steppable` trait states: `successor()` gives the next value, and the `compare` of the `Comparable` it extends says when the loop has reached the end.

The integers are steppable without declaring it, and `String` implements the trait for one-character strings, which step by Unicode code point. A class steps by implementing `Steppable`. A range of any other type is refused:

- a float has no next value: `0.1..1.0` would need a step size, so loop over integers and scale them;
- `bool` has no successor: walk `[false, true]` instead;
- a generic parameter steps only when its bound says it does, so a body looping over a range of `T` bounds `T` by `Steppable`;
- a string written as a bound must be exactly one character: `"a".."ee"` and `"".."b"` have no first or last code point to step between.

Membership is a different question and is unaffected: `x in a..b` tests a value against the bounds of a range of any type `<` orders, floats and `bool` included.

## Messages

- `` Range of '{type}' cannot be stepped: `for` steps through a range with its element type's successor, which the Steppable trait gives ``
- `A range of strings is stepped by code point, so its bound must be exactly one character; "{text}" has {count} characters`

## Help

- `` iterate an integer range and scale it: `for i in 0..n` then `let x = start + (i as float) * step`; `x in a..b` still tests membership in a float range ``
- `` walk the two values in a list: `for b in [false, true]`; `x in a..b` still tests membership in a bool range ``
- `bound the parameter so every type it stands for can be stepped: {declaration}`
- `a '{trait}' may hold a value of any type implementing it, so take both bounds at one type parameter instead: {declaration}`
- `` implement Steppable on '{type}' with `public fn compare(other Self) int` and `public fn successor() Self`, or loop over a list of the values; `x in a..b` still tests membership in a range of any type `<` orders ``
- `` write one character, such as "a"..="z", or loop over the characters of a string with `for c in text` ``

## Before

```miri
use system.io

fn main()
    for x in 0.0..1.0
        println(f"{x}")
```

## After

```miri
use system.io

fn main()
    for i in 0..4
        let x = (i as float) * 0.25
        println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
