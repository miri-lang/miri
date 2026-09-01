## Rule

A GPU operation requires GPU-resident buffers but none are present. GPU kernels launched with `gpu forall` need at least one buffer allocated with `gpu let` to operate on. Without GPU-resident data, there is no reason to launch a GPU kernel.

## Messages

- `'gpu forall' requires at least one gpu-resident buffer; none found (annotate data with 'gpu let')`
- `'gpu forall' capture '{name}' must be gpu-resident.`
- `Annotate the binding with 'gpu let', or copy explicitly: 'gpu let {name}_gpu = {expr}'.`
- `cannot read element of gpu-resident '{name}' from host context; a per-element read would require a readback`
- `cannot call method '{method}' on gpu-resident '{name}' from host context; a buffer-touching method would require a readback`
- `cannot pass gpu-resident '{name}' to host function '{func}'`
- `cannot pass gpu-resident '{name}' to host-only function '{func}' (buffer-touching or host-forcing operations)`
- `passing gpu-resident '{name}' to function '{func}' that indexes, calls methods on, forwards, or returns the array is not yet supported (requires device-handle argument passing)`
- `parameter '{param}' is explicitly marked 'host' but received gpu-resident '{name}'`

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
