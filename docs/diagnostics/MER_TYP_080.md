## Rule

A type set — `f32 or float`, or a name declared for one with `type Real is f32 or float` — stands for any one of its members, and is written only where a type parameter is bounded: `fn sq<T is Real>(x T) T`. Each call binds the parameter to exactly one member, so every value has one concrete type. A set is never the type of a value itself: a binding, a field, a collection element or a function type spelled with a set is refused, because no single representation holds every member. Write one of its members, or take the value through a type parameter the set bounds.

## Messages

- `` '{set}' is a type set, which bounds a type parameter and cannot be the type of a value ``
- `` '{written}' is a type set ({set}), which bounds a type parameter and cannot be the type of a value ``

## Help

- `` use one of its members, or take the value as a type parameter: `fn f<T is {written}>(x T)` ``

## Before

```miri
use system.io

type Real is f32 or float

fn main()
    let y Real = 1.5
    println(f"{y}")
```

## After

```miri
use system.io

type Real is f32 or float

fn twice<T is Real>(x T) T
    return x * 2.0

fn main()
    let y f32 = 1.5
    println(f"{twice(y)}")
```

## Reference

[Type Checker](../reference/types.md)
