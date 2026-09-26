## Rule

Every generic class is compiled once per set of arguments the program reaches it at. Inside a method compiled for one instantiation, a class built at an argument computed from the method's own — `Buf<T, Size * 2>` inside `Buf<T, Size>` — names the instantiation its arguments denote once the parameters are bound. When they denote none, the program is refused rather than run through a body compiled for no instantiation in particular: a value argument must fold to an integer within the signed 128-bit range (every operand must be an integer or a value parameter of the class, and it may not overflow, divide by zero, or use an operator other than `+`, `-`, `*`, `/` and `%`), and a type argument must be one the compiler can name — a closure type taking an `out` or `gpu` parameter, or declaring type parameters of its own, is not. A type nested deeper than the compiler names types to has no name either: every such type would spell alike, so an instance of a generic class, a call to a generic function or a `gpu fn` launch at one is refused wherever it is written, not only inside a method compiled for one instantiation — otherwise all of them would share one compiled body and one drop function. The message names the instance as written and the values its parameters are bound to.

## Messages

- `` instantiating `{instance}` at `{bindings}`: `{argument}` does not fit in a 128-bit integer ``
- `` instantiating `{instance}` at `{bindings}`: `{argument}` is not a compile-time constant ``
- `` instantiating `{instance}` at `{bindings}`: `{argument}` divides by zero ``
- `` instantiating `{instance}` at `{bindings}`: `{argument}` uses an operator a value argument cannot fold ``
- `` instantiating `{instance}`: `{argument}` has no name the compiler can compile a body at ``
- `` {definition} has a type argument with no name the compiler can compile a body at ``

## Help

- `a value argument must fold to an integer within the signed 128-bit range; bound the value, or keep it in a field instead of in the type`
- `instantiate the class at a type the compiler can name; wrap the value in a class or struct and instantiate at that`
- `instantiate at a type the compiler can name; wrap the value in a class or struct and instantiate at that`

## Before

```miri
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v
    fn doubled() T
        let b = Buf<T, Size * 2>(self.v)
        return b.get()

fn main()
    let b = Buf<String, 85070591730234615865843651857942052864>("a" + "b")
    println(b.doubled())
```

## After

```miri
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v
    fn doubled() T
        let b = Buf<T, Size * 2>(self.v)
        return b.get()

fn main()
    let b = Buf<String, 4>("a" + "b")
    println(b.doubled())
```

## Reference

[MIR and Lowering](../reference/mir.md)
