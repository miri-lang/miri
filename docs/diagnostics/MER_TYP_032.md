## Rule

Only values of function type can be called using `()`. This error is raised when attempting to call an expression that has a non-function type (such as an integer, string, or class).

## Messages

- `Expression is not callable: {type}`
- `Type '{type}' is not callable`
- `Cannot call instance method '{method}' on type (did you mean to create an instance?)`

## Before

```miri
fn main()
    let x = 42
    let result = x()
    println(f"{result}")
```

## After

```miri
fn get_value() int
    return 42

fn main()
    let result = get_value()
    println(f"{result}")
```

## Reference

[Type Checker](../reference/types.md)
