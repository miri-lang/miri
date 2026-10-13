## Rule

The workgroup (block) dimensions in a GPU kernel launch must be specified as compile-time literal `Dim3(x, y, z)` expressions. Runtime values or variables are not permitted because the workgroup size is a compile-time constraint in WebGPU and WGSL. The compiler currently reports this refusal as MER_CG_008, with the message `internal compiler error: MIR lowering failed for <function>: gpu fn launch block size must be a compile-time literal Dim3`; the fix is the same.

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
