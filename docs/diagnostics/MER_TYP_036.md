## Rule

A generic type was instantiated with a different number of type arguments than the number of generic parameters it declares. Every generic parameter must be satisfied with a corresponding type argument at instantiation.

## Messages

- `Class '{signature}' expects {count} generic arguments, got {got}`
- `Generic argument count mismatch: expected {count}, got {got}`
- `'{name}' expects {count} {noun}, got {got}`

## Before

```miri
class Pair<T, U>
    var first T
    var second U

fn main()
    let p = Pair<int>(first: 1, second: 2)
    println(f"{p.first}")
```

## After

```miri
class Pair<T, U>
    var first T
    var second U

fn main()
    let p = Pair<int, int>(first: 1, second: 2)
    println(f"{p.first}")
```

## Reference

[Type Checker](../reference/types.md)
