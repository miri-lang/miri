## Rule

GPU kernel launches specify the workgroup (block) dimensions as `Dim3(x, y, z)`. All three dimensions must be positive integers (greater than zero). This error is raised when any dimension is zero or negative.

## Before

```miri
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(16, 0, 1))
```

## After

```miri
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(16, 16, 1))
```

## Reference

[Type Checker](../reference/types.md)
