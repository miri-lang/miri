## Rule

Inside a trait, `Self` names the class implementing it. A value whose type is a trait holds some class that implements the trait, and does not say which: the compiler only learns that when the program runs. So a method whose parameter is `Self` cannot be called through such a value, because nothing checks that the argument is the same class as the value it is called on. The same holds for a method that returns `Self` inside another type (`List<Self>`, `Walk<T, Self>`), which is a different type for every class, and for an operator whose method takes `Self`, such as `==` through `Equatable` or `<` through `Comparable`.

For the same reason a value of a trait type does not stand for a type parameter bounded by a trait with such a method: `check(a, b)` with `a` and `b` both declared `Same` is refused when `check` is `fn check<T implements Same>(p T, q T)`, since the two values may be different classes. A trait whose methods only return a bare `Self` or `Self?` is still met by its own values.

Take the values at one type parameter bounded by the trait instead, `fn check<T implements Same>(p T, q T)`. Each call to `check` then names the one class both values are, and `p.same(q)` is compiled for it. A method returning a bare `Self` or `Self?` may still be called through a trait value: the result is a value of the trait type.

## Messages

- `` the method `{method}` takes a `Self`, and a value of the trait `{trait}` does not say which class that is ``
- `` the method `{method}` returns `Self` inside another type, and a value of the trait `{trait}` does not say which class that is ``
- `` `{operator}` runs `{method}`, which takes a `Self`, and a value of the trait `{trait}` does not say which class that is ``
- `` a value of the trait `{trait}` cannot stand for a type parameter bounded by `{bound}`: the method `{method}` takes a `Self`, and the value does not say which class that is ``

## Help

- `` take the values at one type parameter bounded by the trait, as in `fn f<T implements {trait}>(p T, q T)`, so each call is compiled for the class they share ``
- `` hand the values over at their class, or take them at a type parameter of the caller's own, as in `fn f<T implements {bound}>(p T, q T)` ``

## Before

```miri
trait Same
    fn same(other Self) bool

class A implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other A) bool
        return self.x == other.x

fn check(p Same, q Same) bool
    return p.same(q)

fn main()
    let ok = check(A(1), A(1))
```

## After

```miri
trait Same
    fn same(other Self) bool

class A implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other A) bool
        return self.x == other.x

fn check<T implements Same>(p T, q T) bool
    return p.same(q)

fn main()
    let ok = check(A(1), A(1))
```

## Reference

[Type Checker](../reference/types.md)
