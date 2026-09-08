## Rule

The program called `panic`, which stops it where it stands and reports the message it was given. This is the program saying it has reached a state it has no way to continue from, so the fix is to remove the reason rather than the call — handle the case the panic guards, or answer with a value the caller can inspect.

## Before

```miri
fn half(n int) int
    if n % 2 != 0
        panic("odd number")
    return n / 2

fn main()
    println(f"{half(7)}")
```

## After

```miri
fn half(n int) int?
    if n % 2 != 0
        return null
    return n / 2

fn main()
    match half(7)
        null: println("not halvable")
        value: println(f"{value}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
