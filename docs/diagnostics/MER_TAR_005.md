## Rule

A parallel construct (such as `forall`) contains an invalid pattern. This code covers a family of violations: loop-carried accumulators that make iterations order-dependent, incorrect number of loop variables, or other restrictions on how `forall` may be structured. The `forall` keyword requires iterations to be independent.

## Before

```miri
use system.gpu

fn main()
    var sum = 0
    gpu forall i in 0..4
        sum = sum + i
```

## After

```miri
use system.gpu

fn main()
    gpu let arr = [0, 1, 2, 3]
    let sum = arr.reduce(0, fn(acc i32, x i32) i32: acc + x)
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
