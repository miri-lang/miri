## Rule

A value a range was asked to step from has no next value. A `for` loop over a range of a type implementing `Steppable` asks `steppable()` of both bounds before its first pass, so a bound the type cannot step from stops the program before the body runs even once. A `String` steps only when it is exactly one character: a bound computed at run time as `"ab"`, or `""`, stops the loop here. Asking a string for its `successor()` directly stops the same way when it is not one character, or when it holds U+10FFFF, the last Unicode code point.

A bound written as a string literal is checked when the program is compiled instead (MER_TYP_086). Check a computed bound with `steppable()` before looping over it.

## Messages

- `Value Cannot Be Stepped`
- `a range bound cannot be stepped: its steppable() is false`
- `"{text}" has no successor: only a one-character string steps` (a string longer than 32 characters is shown by its first 32, followed by `…`)
- `U+10FFFF has no successor: it is the last Unicode code point`

## Before

```miri
use system.io

fn main()
    var start = "a"
    start = start + "b"
    for c in start.."e"
        println(c)
```

## After

```miri
use system.io

fn main()
    var start = "a"
    start = start + "b"
    if start.steppable()
        for c in start.."e"
            println(c)
    else
        println(f"cannot step from {start}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
