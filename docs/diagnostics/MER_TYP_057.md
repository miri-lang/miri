## Rule

A trait definition violates structural requirements. This code covers multiple family members: duplicate method signatures, invalid method declarations, conflicting default implementations, or other constraint violations in trait definitions.

## Messages

- `'{name}' is not a trait`
- `Trait '{name}' is not defined`
- `Class '{class}' must implement method '{method}' from trait '{trait}'`

## Help

- `'{name}' is {kind} — only traits can be used with 'implements'`

## Before

```miri
class Logger
    fn log(msg String)
        println(msg)

class FileLogger implements Logger
    fn log(msg String)
        println(msg)

fn main()
    let l = FileLogger()
    l.log("hi")
```

## After

```miri
trait Logger
    fn log(msg String)

class FileLogger implements Logger
    fn log(msg String)
        println(msg)

fn main()
    let l = FileLogger()
    l.log("hi")
```

## Reference

[Type Checker](../reference/types.md)
