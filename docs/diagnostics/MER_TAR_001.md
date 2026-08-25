## Rule

A shuffle operation specifies an offset that exceeds the maximum subgroup size (128). The shuffle offset must be a compile-time integer literal in the range 0 to 128.

## Before

```miri
use system.gpu

gpu fn my_kernel()
    gpu let a = [1, 2, 3, 4]
    let x = a[0]
    let shuffled = shuffle(x, 200)
```

## After

```miri
use system.gpu

gpu fn my_kernel()
    gpu let a = [1, 2, 3, 4]
    let x = a[0]
    let shuffled = shuffle(x, 2)
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
