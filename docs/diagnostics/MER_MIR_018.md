## Rule

Every function, method, generic instantiation and synthesized helper is compiled to a body linked under a name built from what it stands for. That spelling separates its parts with `.` and `$`, which no identifier or type argument can contain, so two different definitions never share a name — `Point.norm` and a free function `Point_norm`, or the generic `pick<int>` and a function named `pick__int`, each keep their own body. This error is the guard behind that rule: if two different definitions were ever to reach one compiled name, the build is refused rather than linked with one body standing in for both, and the message names both definitions as the source writes them. No program is expected to raise it; one that does has found a compiler bug.

## Messages

- `` {existing} and {incoming} compile to the same symbol `{name}` ``

## Help

- `rename one of the two definitions so that their compiled names differ`

## Before

```miri
use system.io

class Point
    x int
    fn norm() int
        return self.x

fn Point_norm(p Point) int
    return p.x * 2

fn main()
    let p = Point(x: 1)
    println(f"{p.norm()} {Point_norm(p)}")
```

## After

```miri
use system.io

class Point
    x int
    fn norm() int
        return self.x

fn doubled_norm(p Point) int
    return p.x * 2

fn main()
    let p = Point(x: 1)
    println(f"{p.norm()} {doubled_norm(p)}")
```

## Reference

[MIR and Lowering](../reference/mir.md)
