## Rule

The offset passed to `kernel.warp.shuffle_down` exceeds the maximum subgroup size (128), or is not a compile-time integer literal. The offset must be a literal in the range 0 to 128.

## Messages

- `shuffle offset {offset} exceeds the maximum subgroup size (128)`
- `shuffle offset must be a compile-time literal`

## Before

```miri
use system.collections.array

gpu fn shift(src Array<int, 32>, dst out Array<int, 32>)
    let lane = kernel.warp.lane_id
    dst[lane] = kernel.warp.shuffle_down(src[lane], 200)

fn main()
    gpu let src = Array<int, 32>()
    gpu var dst = Array<int, 32>()
    shift(src, dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
```

## After

```miri
use system.collections.array

gpu fn shift(src Array<int, 32>, dst out Array<int, 32>)
    let lane = kernel.warp.lane_id
    dst[lane] = kernel.warp.shuffle_down(src[lane], 2)

fn main()
    gpu let src = Array<int, 32>()
    gpu var dst = Array<int, 32>()
    shift(src, dst).launch(Dim3(1, 1, 1), Dim3(32, 1, 1))
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
