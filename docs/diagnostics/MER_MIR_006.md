## Rule

A `continue` statement appears outside of any loop context. The `continue` keyword can only be used inside a `while`, `for`, or `forall` loop to skip to the next iteration.

## Before

```miri
fn main()
    continue
```

## After

```miri
fn main()
    var sum = 0
    while sum < 5
        sum = sum + 1
        if sum == 3: continue
        println(sum)
```

## Reference

[MIR and Lowering](../reference/mir.md)
