## Rule

A parallel construct (such as `forall`) contains an invalid pattern. This code covers a family of violations: loop-carried accumulators that make iterations order-dependent, incorrect number of loop variables, or other restrictions on how `forall` may be structured. The `forall` keyword requires iterations to be independent.

## Messages

- `loop-carried accumulator '{name}' makes 'forall' iterations order-dependent; 'forall' requires independent iterations (reductions are not yet supported)`
- `forall: expected 1, 2, or 3 loop variables, got {count}`
- `2D forall requires two comma-separated ranges`
- `2D forall requires exactly two ranges`
- `3D forall requires three comma-separated ranges`
- `3D forall requires exactly three ranges`
- `forall dimension {dim}: range must be a bounded numeric range like '0..n'`
- `forall dimension {dim}: range start must be Int, got {type}`
- `forall dimension {dim}: range end must be Int, got {type}`
- `'gpu forall' requires a bounded numeric range like 'a..b' or 'a..=b'`
- `'gpu forall' range {dim} start must be Int, got {type}`
- `'gpu forall' range {dim} end must be Int, got {type}`
- `'gpu forall' requires at least one gpu-resident buffer; none found (annotate data with 'gpu let')`
- `gpu frame requires exactly 1 loop variable`
- `'gpu frame' requires a bounded numeric range like 'a..b' or 'a..=b'`
- `'gpu frame' requires Int-literal range start`
- `'gpu frame' range end must be Int, got {type}`
- `gpu frame block body must be a block statement`
- `'gpu frame' block must contain at least one 'gpu forall' pass`
- `'gpu frame' block may only contain 'gpu forall' passes or a literal-count 'for _ in 0..k' repeat around them`
- `'gpu frame' pass must write at least one gpu buffer`
- `'gpu frame' pass creates a data race: buffer '{name}' is both read and written in the same pass (use a separate ping-pong buffer)`

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
