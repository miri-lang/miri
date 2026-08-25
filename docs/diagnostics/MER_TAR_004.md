## Rule

A barrier synchronization call has invalid control flow. Barriers in GPU code require all threads in a workgroup to execute the same barrier; control flow that makes some threads skip the barrier (e.g., inside a conditional branch that not all threads take) is not allowed.

## Before

```miri
use system.gpu

gpu fn my_kernel()
    gpu let a = [1, 2, 3, 4]
    if a[0] > 2
        barrier()
```

## After

```miri
use system.gpu

gpu fn my_kernel()
    gpu let a = [1, 2, 3, 4]
    barrier()
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
