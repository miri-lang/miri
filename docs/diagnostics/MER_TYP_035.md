## Rule

A name exists in the type system but is not visible from the current scope due to visibility restrictions (e.g., private module members accessed from another module). The compiler enforces strict visibility rules to prevent unintended access to implementation details.

## Messages

- `Field '{field}' of class '{class}' is {visibility} and cannot be accessed from here`
- `Method '{method}' of class '{class}' is {visibility} and cannot be accessed from here`
- `Type '{name}' is not visible`
- `'{name}' is not visible`

## Before

```miri
class Account
    private balance int

    fn init(start int)
        self.balance = start

fn main()
    let a = Account(start: 10)
    println(f"{a.balance}")
```

## After

```miri
class Account
    private balance int

    fn init(start int)
        self.balance = start

    fn current() int
        return self.balance

fn main()
    let a = Account(start: 10)
    println(f"{a.current()}")
```

## Reference

[Type Checker](../reference/types.md)
