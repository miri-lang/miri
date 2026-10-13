## Rule

A division or modulo operand in a `gpu forall` body is an integer literal outside the 32-bit signed range representable on the GPU. On the GPU, `int` is 32-bit, so a larger literal would be silently narrowed by the WGSL backend, and the kernel would divide by a different value than the source spells. Use a value that fits in 32 bits.

## Messages

- `GPU '/' or '%' operand is an integer literal outside the 32-bit range representable on the GPU; the WGSL backend would silently narrow it to i32 and divide by a different value`

## Help

- `cast or clamp the operand into i32 range (for example 'x as i32') before the division, or use a value that fits in 32 bits`

## Before

```miri
use system.collections.array

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / 5000000000
```

## After

```miri
use system.collections.array

fn main()
    gpu var out = Array<int, 4>()
    gpu forall i in 0..4
        out[i] = i / 50000
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
