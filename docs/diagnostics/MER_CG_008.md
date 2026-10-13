## Rule

An internal error occurred while lowering the program or generating its code. When the compiler itself fails, the message names the stage and the help asks for a report: send the program that triggered it.

The same code is how a refusal made while lowering a function body reaches you. The message then reads `internal compiler error: MIR lowering failed for <function>: <reason>`, and the reason names a construct in your program that lowering refused: a launch block size that is not a compile-time literal `Dim3`, a block dimension of zero, a host-resident array passed to a `gpu fn`, or an atomic operation on a buffer that is not `Array<Atomic<u32|i32>, N>`. Change the construct the reason names. A reason that names nothing in your program is a compiler bug.

## Messages

- `internal compiler error: {detail}`
- `Internal codegen error: {detail}`

## Help

- `Please report this at {url}`

## Before

```miri
use system.collections.array

gpu fn fill(data out Array<f32, 16>)
    data[kernel.thread_idx.x] = 1.0

fn main()
    gpu var data = Array<f32, 16>()
    let width = 16
    fill(data).launch(Dim3(1, 1, 1), Dim3(width, 1, 1))
```

## After

```miri
use system.collections.array

gpu fn fill(data out Array<f32, 16>)
    data[kernel.thread_idx.x] = 1.0

fn main()
    gpu var data = Array<f32, 16>()
    fill(data).launch(Dim3(1, 1, 1), Dim3(16, 1, 1))
```

## Reference

[Code Generation](../reference/codegen.md)
