## Rule

A function declaration has an invalid signature. This covers multiple family members: incorrect parameter types, incompatible return types, invalid generic constraints, or mismatched out-parameter declarations. The signature must be well-formed and consistent with any overrides or implementations.

## Messages

- `Missing return statement`
- `conflict: parameter '{param}' cannot be explicitly marked 'host' in a gpu function`
- `Regex literals cannot be used inside a GPU function; use Regex.compile() at host level and pass it as a parameter`
- `parameter '{param}' is explicitly marked 'gpu' but received host-resident argument`
- `expected mutable variable for 'out' parameter '{param}', but got a non-variable expression`

## Before

```miri
fn add(a int, b int) int
    let x = a + b

fn main()
    println("ok")
```

## After

```miri
fn add(a int, b int) int
    return a + b

fn main()
    println("ok")
```

## Reference

[Type Checker](../reference/types.md)
