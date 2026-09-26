## Rule

Every function, method and generic instantiation is compiled to a body linked under one name. That name joins the identifiers the source writes with `_` and `__` — a method `norm` of `Point` is `Point_norm`, the generic `pick` at `int` is `pick__int` — and an identifier may itself contain those spellings, so two different definitions can come out under the same name. The program is refused rather than linked with one body standing in for both: a call to either would otherwise run whichever definition was compiled first. The message names both definitions as the source writes them and the name they share.

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
