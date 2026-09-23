## Rule

A program built with `--target web-gpu` does something a web bundle cannot carry. The browser runs the program's kernels and nothing else: no host statement executes there. So every GPU buffer's contents must be known when the bundle is built (a literal of constants, or a sized `Array<T, N>()` constructor), every launch must dispatch a grid fixed at build time, each kernel must be launched on exactly one set of buffers, and each buffer must hold elements the browser runtime can store (`i32`, `u32` or `f32`). A program that breaks one of these still builds for the native target.

## Messages

- `gpu buffer '{name}' cannot be evaluated when the bundle is built; a web bundle runs no host code, so its contents must be a literal or a sized constructor`
- `the bound of this parallel loop is known only at run time; a web bundle dispatches a grid fixed when the bundle is built`
- `the grid of this launch is not known when the bundle is built; a web bundle dispatches a grid fixed when the bundle is built`
- `gpu buffer '{name}' holds {element} elements, which a web bundle cannot carry; the browser runtime stores i32, u32 and f32 elements only`
- `kernel {kernel} is never launched, so a web bundle cannot tell which buffers it binds`
- `this launch passes kernel {kernel} different buffers than an earlier one; a web bundle binds each kernel to one set of buffers`

## Before

```miri
fn make(k f32) [f32; 4]
    return [k, k + 1.0, k + 2.0, k + 3.0]

gpu let a = make(10.0)
gpu var dst = [0.0, 0.0, 0.0, 0.0]

forall i in 0..4
    dst[i] = a[i] * 2.0
```

Built with `miri build --target web-gpu`: `make` is host code, so the bundle cannot know what `a` holds.

## After

```miri
gpu var a = [0.0, 0.0, 0.0, 0.0]
gpu var dst = [0.0, 0.0, 0.0, 0.0]

forall i in 0..4
    a[i] = 10.0 + i as f32

forall i in 0..4
    dst[i] = a[i] * 2.0
```

## Reference

[Target-Specific Capabilities and Restrictions](../reference/targets.md)
