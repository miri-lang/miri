## Rule

Every generic class is compiled once per set of arguments the program reaches it at, so each instantiation's methods handle its own values — a `String` counted, a struct kept whole, a scalar at its width. A method that builds its own class at an argument grown from its own — `Impl<Wrap<T>>` inside `Impl<T>`, `Buf<T, Size + 1>` inside `Buf<T, Size>` — and reaches that instance's methods again asks for a new instantiation on every call, and no finite set of compiled bodies covers the program. Whether the recursion stops is decided while the program runs, so the compiler bounds each instantiation instead: an instance whose type arguments nest more than 32 type constructors deep (`Vec<List<List<String>>>` is three), an instance whose type arguments are built from more than 1024 types (`(int, (int, String))` is five; a type that doubles on every call, `(T, T)` inside a body at `T`, passes this bound long before the depth one), or a class needed at more than 256 value arguments, is refused. The bounds hold for a generic function or method calling itself at a grown type argument as they do for a class. Both bounds depend only on the set of instances the program needs, never on the order it declares or reaches them. The note names the chain of instances that grew. Bound the recursion by type, or keep one type and carry the changing part as a value.

## Messages

- `` instantiating `{instance}` nests its type argument {depth} levels deep ``
- `` instantiating `{instance}` builds its type argument from more types than one instance may hold (1024) ``
- `` instantiating `{instance}`: `{class}` needs more than 256 instantiations of `{parameter}` ``

## Help

- `the type argument grows on every call through the trait; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)`
- `the type argument grows on every call; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)`
- `the value argument changes on every call; bound the recursion, or keep one value (e.g. store the size in a field instead of in the type)`

## Before

```miri
use system.io

trait Op<T>
    fn depth(n int) int

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Impl<T> implements Op<T>
    fn depth(n int) int
        if n == 0
            return 0
        let inner Op<Wrap<T>> = Impl<Wrap<T>>()
        return 1 + inner.depth(n - 1)

fn main()
    let o Op<int> = Impl<int>()
    println(f"{o.depth(3)}")
```

## After

```miri
use system.io

trait Op<T>
    fn depth(n int) int

class Impl<T> implements Op<T>
    fn depth(n int) int
        if n == 0
            return 0
        let inner Op<T> = Impl<T>()
        return 1 + inner.depth(n - 1)

fn main()
    let o Op<int> = Impl<int>()
    println(f"{o.depth(3)}")
```

## Reference

[MIR and Lowering](../reference/mir.md)
