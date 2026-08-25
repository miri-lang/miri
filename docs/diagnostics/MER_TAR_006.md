## Rule

A GPU operation requires GPU-resident buffers but none are present. GPU kernels launched with `gpu forall` need at least one buffer allocated with `gpu let` to operate on. Without GPU-resident data, there is no reason to launch a GPU kernel.

## Before

```miri
use system.gpu

fn main()
    let data = [1, 2, 3, 4]
    gpu forall i in 0..4
        let x = i + 1
```

## After

```miri
use system.gpu

fn main()
    gpu let data = [1, 2, 3, 4]
    gpu forall i in 0..4
        data[i] = data[i] + 1
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
