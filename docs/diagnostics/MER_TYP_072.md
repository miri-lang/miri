## Rule

A parameter is never read in the body of the function that declares it. The caller computes an argument for it on every call and the body throws it away.

Either the body should be using it, or the parameter should go — and removing it changes the signature, so every call site changes with it. That is why this is reported and not repaired automatically.

A parameter whose name begins with `_` is never reported. That spelling is what to write when the signature is fixed by something outside the function — a trait it implements, a callback shape it is passed to — and this particular body has no use for the value.

A declaration with no body (a trait method signature, an abstract method, a `runtime` or `intrinsic` binding) has no unused parameters: there is no body that could have read them.

## Messages

- `Unused parameter: '{name}' is never read in the body of '{function}'`

## Before

```miri
fn greet(name String) String
    "hello"

fn main()
    println(greet("world"))
```

## After

```miri
fn greet(name String) String
    f"hello {name}"

fn main()
    println(greet("world"))
```

## Reference

[Type Checker](../reference/types.md)
