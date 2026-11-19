## Rule

The copy trait is the trait the standard library declares `@copy_trait`: a collection copies an element whose class implements it through the element's own `clone()`, and a copy of a resource that does not is refused. The compiler finds that trait by the marker, never by its name, so the marker is accepted only on a trait that declares the `clone() Self` a copy calls: an instance method taking nothing and answering the implementing type.

One trait is the copy trait, so only one trait carries the marker. The standard library's `Cloneable` does; a program that wants a trait of the same shape declares it without the marker, and it is an ordinary trait.

## Messages

- `` '{name}' is marked `@copy_trait`, but it declares no `clone() Self`, the method a copy calls ``
- `` '{name}' is marked `@copy_trait`, and '{existing}' already is: one trait is the copy trait ``

## Help

- `` declare `fn clone() Self` in '{name}', or remove `@copy_trait` ``
- `` remove `@copy_trait` from '{name}' ``

## Before

```miri
use system.io

@copy_trait
trait Duplicable
    fn clone() Self

fn main()
    println("one copy trait")
```

## After

```miri
use system.io

trait Duplicable
    fn clone() Self

fn main()
    println("one copy trait")
```

## Reference

[Type Checker](../reference/types.md)
