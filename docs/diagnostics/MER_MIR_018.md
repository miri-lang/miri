## Rule

Every function, method, generic instantiation and synthesized helper is compiled to a body linked under a name built from what it stands for. That spelling separates its parts with `.` and `$`, which no identifier or type argument can contain, so on the host two different definitions never share a name — `P.norm` and a free function `P_norm` each keep their own body. Code that runs on the GPU is different: a kernel and every function it calls are declared in a WGSL module, whose names admit identifier characters only. Functions and methods are spelled there so that no two of them meet — `P.norm` is declared as `m__t1P4norm`, apart from `P_norm` — but a kernel the compiler makes for a `forall` is declared under a plain name such as `miri_gpu_forall_0`, and a program function may be named exactly that. When two different definitions reached from GPU code are declared under one such name, the build is refused rather than launching a kernel whose module declares that name twice. The message names both definitions as the source writes them and the kernel-side name they share.

## Messages

- `` {existing} and {incoming} compile to the same symbol `{name}` ``
- `` {existing} and {incoming} are both reached from GPU code, where both are declared as `{name}` ``

## Help

- `rename one of the two definitions so that their compiled names differ`

## Before

```miri
use system.io

fn miri_gpu_forall_0(x int) int
    return x + 1

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu forall i in 0..4
        a[i] = miri_gpu_forall_0(a[i])
    let h = a
    println(f"{h[0]}")
```

## After

```miri
use system.io

fn bump(x int) int
    return x + 1

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu forall i in 0..4
        a[i] = bump(a[i])
    let h = a
    println(f"{h[0]}")
```

## Reference

[MIR and Lowering](../reference/mir.md)
