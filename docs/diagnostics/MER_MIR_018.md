## Rule

Every function, method, generic instantiation and synthesized helper is compiled to a body linked under a name built from what it stands for. That spelling separates its parts with `.` and `$`, which no identifier or type argument can contain, so on the host two different definitions never share a name — `P.norm` and a free function `P_norm` each keep their own body. Code that runs on the GPU is different: a kernel and every function it calls are declared in a WGSL module, whose names admit identifier characters only, so there the parts of a name are joined with `_` and `P.norm` is declared as `P_norm`. When two different definitions reached from GPU code are declared under one such name, the build is refused rather than launching a kernel whose module declares that name twice. The message names both definitions as the source writes them and the kernel-side name they share.

## Messages

- `` {existing} and {incoming} compile to the same symbol `{name}` ``
- `` {existing} and {incoming} are both reached from GPU code, where both are declared as `{name}` ``

## Help

- `rename one of the two definitions so that their compiled names differ`

## Before

```miri
use system.io

class P
    static fn norm(x int) int
        return x + 1

fn P_norm(x int) int
    return x + 2

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu forall i in 0..4
        a[i] = P.norm(a[i]) + P_norm(a[i])
    let h = a
    println(f"{h[0]}")
```

## After

```miri
use system.io

fn plus_one(x int) int
    return x + 1

fn plus_two(x int) int
    return x + 2

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu forall i in 0..4
        a[i] = plus_one(a[i]) + plus_two(a[i])
    let h = a
    println(f"{h[0]}")
```

## Reference

[MIR and Lowering](../reference/mir.md)
