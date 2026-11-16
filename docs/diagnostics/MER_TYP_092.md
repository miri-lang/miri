## Rule

`T?` is the enum the standard library declares `@optional`, and `T?` and `Option<T>` are one type. The compiler keeps its own layout for that enum, laying an optional out by its payload rather than as an enum, so the marker is accepted only on an enum whose shape the layout keeps: one type parameter, exactly two variants, one without a payload and one carrying exactly that parameter, declared at the top level of a module, with a closed variant set and no `drop`, `equals` or `hash` of its own.

`T?` names one type, so only one enum carries the marker. The standard library's `Option` does; a program that wants an enum of the same shape declares it without the marker, and it is an ordinary enum.

## Messages

- `` '{name}' is marked `@optional`, but {reason} ``
- `` '{name}' is marked `@optional`, and '{existing}' already is: `T?` names one type ``

## Help

- `` declare `@optional enum Name<T>` at the top level, with one variant without a payload and one carrying `T`, and no `drop`, `equals` or `hash` ``
- `` remove `@optional` from '{name}' ``

## Before

```miri
use system.io

@optional
enum Maybe<T>
    Nothing
    Just(T)

fn main()
    let m = Maybe.Just(4)
    match m
        Maybe.Just(n): println(f"{n}")
        Maybe.Nothing: println("nothing")
```

## After

```miri
use system.io

enum Maybe<T>
    Nothing
    Just(T)

fn main()
    let m = Maybe.Just(4)
    match m
        Maybe.Just(n): println(f"{n}")
        Maybe.Nothing: println("nothing")
```

## Reference

[Type Checker](../reference/types.md)
