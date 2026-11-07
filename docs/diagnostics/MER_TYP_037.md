## Rule

A generic type argument does not satisfy the constraint declared for that generic parameter. The type checker verifies that all type arguments conform to their parameter constraints at instantiation sites.

A list of several bounds joined with `and` is also checked where it is declared, since no argument could meet a list that says a thing twice or asks for the impossible: a bound written twice, a type set beside another bound, two classes (a type extends at most one), and two bounds declaring one member with different signatures are refused there.

## Messages

- `Type {type} does not satisfy constraint {kind} {constraint}`
- `{type} is not one of the types {set} accepts: {members}`
- `` the bound `{bound}` is written twice for `{parameter}` ``
- `` a type set (`is {set}`) is not combined with other bounds ``
- `` `{parameter}` names two classes, `{class}` and `{class}`, in its bounds ``
- `` the bounds `{bound}` and `{bound}` of `{parameter}` both declare `{member}` with different signatures, so no type can meet both ``

## Before

```miri
trait Named
    fn name() String

fn greet<T implements Named>(x T) String
    return x.name()

fn main()
    println(greet(5))
```

## After

```miri
trait Named
    fn name() String

class Robot implements Named
    fn name() String
        return "R2"

fn greet<T implements Named>(x T) String
    return x.name()

fn main()
    println(greet(Robot()))
```

## Reference

[Type Checker](../reference/types.md)
