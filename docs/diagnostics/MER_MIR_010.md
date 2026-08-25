## Rule

A GPU launch call has an incorrect number of arguments. GPU launch operations require exactly two arguments: grid dimensions and block dimensions, both specified as `Dim3` structures.

## Before

```miri
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch()
```

## After

```miri
use system.gpu

gpu fn my_kernel()
    let x = 1

fn main()
    my_kernel().launch(Dim3(1, 1, 1), Dim3(1, 1, 1))
```

## Reference

[MIR and Lowering](../reference/mir.md)
