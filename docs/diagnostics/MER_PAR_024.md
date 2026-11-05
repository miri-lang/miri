## Rule

Each parameter in a list has its own name: the value parameters of a function,
method or lambda, and the type parameters of a generic declaration.

A list that names one parameter twice could only ever mean one of them in the
body, so `fn dist(x float, x float)` would read the second argument wherever
it says `x`. The parser refuses the second name instead. A parameter named
`_` names nothing and may repeat.

## Messages

- `Parameter '{name}' is declared twice`
- `Type parameter '{name}' is declared twice`

## Help

- `Only one of the two could be read, so give each parameter its own name.`

## Before

```miri
use system.io

fn dist(x float, x float) float
    return x

fn main()
    println(f"{dist(1.0, 2.0)}")
```

## After

```miri
use system.io

fn dist(x float, y float) float
    return y - x

fn main()
    println(f"{dist(1.0, 2.0)}")
```

## Reference

[Parser](../reference/parser.md)
