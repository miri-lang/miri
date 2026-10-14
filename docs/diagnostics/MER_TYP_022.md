## Rule

The workgroup (block) dimensions in a GPU kernel launch must be specified as compile-time literal `Dim3(x, y, z)` expressions. Runtime values or variables are not permitted because the workgroup size is a compile-time constraint in WebGPU and WGSL.

## Messages

- `` a launch's block size is fixed when its kernel is compiled, but {found} is not {expected} ``

## Help

- `` write the block size with integer literals, e.g. `Dim3(16, 16, 1)` ``

## Before

```miri
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    var block_size = 16
    my_kernel().launch(Dim3(1, 1, 1), Dim3(block_size, 16, 1))
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
