## Rule

A method may declare type parameters of its own, `fn pick<T>(x T) T`, and each call binds them, so a body is compiled for each set of type arguments the calls use. A value whose type is a trait is called through a vtable: the call reaches whichever class the value holds, through one slot per method. One slot cannot hold a body for every type argument, so a generic method of a trait is not called through a value of the trait type.

Take the value at a type parameter bounded by the trait instead, `fn use<G implements Gen>(g G)`. Each call to `use` then names the class it passes, and `g.pick(5)` is compiled for that class and for `int`. Inside a trait's own default methods, `self` names the implementing class the same way, so a default may call a generic method on `self`.

For the same reason a call is refused where it binds such a type parameter to the trait type itself: `through(g)` with `g` declared as `Gen` would compile `g.pick(5)` for the trait. The rule follows the parameter through every function or class that hands it on, and is reported at the call that binds it to the trait.

## Messages

- `` the generic method `{method}` is called through a value of the trait `{trait}`, which has no body for every type argument ``
- `` `{function}` calls the generic method `{method}` through its type parameter `{parameter}`, which this call binds to the trait `{trait}`; a value of a trait type has no body for every type argument ``

## Help

- `` take the value as `<G implements {trait}>` so each call is compiled for its type ``
- `` bind `{parameter}` to a class that implements `{trait}`: take the value as `<G implements {trait}>` so each call is compiled for its type ``

## Before

```miri
trait Gen
    fn pick<T>(x T) T

class Impl implements Gen
    public fn pick<T>(x T) T
        return x

fn main()
    let g Gen = Impl()
    let n = g.pick(5)
```

## After

```miri
trait Gen
    fn pick<T>(x T) T

class Impl implements Gen
    public fn pick<T>(x T) T
        return x

fn through<G implements Gen>(g G) int
    return g.pick(5)

fn main()
    let n = through(Impl())
```

## Reference

[Type Checker](../reference/types.md)
