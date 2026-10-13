## Rule

A function was called with an argument its parameter's guard refuses, so the program stopped before the body ran. A guard is written after the parameter's type (`n int > 0`, `c String in valid`, `d int not 0`) and states what every caller must pass; the message names the function, the parameter and the guard. Pass a value the guard admits, or check the value before the call and take another path when it would not be admitted.

## Messages

- `Invalid Operand`

## Before

```miri
fn half(n int > 0) int
    return n / 2

fn main()
    println(f"{half(0)}")
```

## After

```miri
fn half(n int > 0) int
    return n / 2

fn main()
    let n = 0
    if n > 0
        println(f"{half(n)}")
    else
        println("nothing to halve")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
