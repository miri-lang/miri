## Rule

A `break` statement appears outside of any loop context. The `break` keyword can only be used inside a `while`, `for`, or `forall` loop to terminate the loop.

## Before

```miri
fn main()
    break
```

## After

```miri
fn main()
    var sum = 0
    while sum < 5
        sum = sum + 1
        break
```

## Reference

[MIR and Lowering](../reference/mir.md)
