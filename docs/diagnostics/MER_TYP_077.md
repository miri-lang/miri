## Rule

A value argument — the `3` in `Buf<T, 3>`, the `Size - 1` in `Buf<T, Size - 1>` — counts something each instance of the class holds, so it is at least 1. A zero or a negative count has no use in a type: a count that may be zero or negative belongs in a field, passed to the constructor like any other value. A value argument written as a literal or a `const` is refused where it is written. The type `[T; 0]` of the empty array literal `[]` is not a written class argument and stays valid. One computed from a value parameter is known only at an instantiation, and one that reaches zero or below there is refused at that instance with `MER_MIR_017`.

## Messages

- `` the value argument of `{class}` is {value}; a value argument must be greater than zero ``

## Help

- `` a value argument counts what each instance holds, so it is at least 1; pass a count that may be zero or negative to the constructor instead ``

## Before

```miri
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let b = Buf<String, 0>("a" + "b")
    println(b.get())
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

fn main()
    let b = Buf<String, 1>("a" + "b")
    println(b.get())
```

## Reference

[Type Checker](../reference/types.md)
