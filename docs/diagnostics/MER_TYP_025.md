## Rule

When a variable is declared with an explicit `Option<T>` type but is initialized with a non-null value (not `Option.None`), the `Option` wrapper is redundant. The compiler infers the correct type from the initializer.

## Messages

- `Unnecessary optional declaration for variable '{name}'`

## Before

```miri
fn main()
    let x Option<int> = 42
    println(f"{x}")
```

## After

```miri
fn main()
    let x = 42
    println(f"{x}")
```

## Reference

[Type Checker](../reference/types.md)
