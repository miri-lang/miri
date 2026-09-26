## Rule

A value argument — the `3` in `Buf<T, 3>`, the `Size + K` in `Buf<T, Size + K>` — selects which instantiation of a generic class a program builds, and every instantiation is compiled before the program runs. So each operand of a value argument must be known then: an integer literal, a `const` or an immutable binding of an integer literal, or a generic parameter of the enclosing declaration, which the instantiation it runs at binds to a value. A binding computed while the program runs, a call, or a field read has no value until then, and an argument built from one names no instantiation. The program is refused where the argument is written.

## Messages

- `` `{operand}` is not a compile-time constant, so the value argument of `{class}` names no instantiation ``

## Help

- `` build a value argument from integer literals, `const`s and the value parameters in scope; keep a value known only while the program runs in a field instead of in the type ``

## Before

```miri
use system.io

fn seven() int
    return 7

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let m = seven()
    let b = Buf<String, 2 + m>("a" + "b")
    println(b.get())
```

## After

```miri
use system.io

const M = 7

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let b = Buf<String, 2 + M>("a" + "b")
    println(b.get())
```

## Reference

[Type Checker](../reference/types.md)
