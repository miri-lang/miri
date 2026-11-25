## Rule

A binding a `match` arm's pattern reads out of the matched value borrows that value's payload: the matched place keeps it alive. Assigning the matched place, or a place containing it, handing either to an `out` parameter, or calling on either a method that writes its receiver replaces the value and releases the payload the binding reads. So the write ends every payload binding read out of what it writes, and a later use of one whose type reaches a `drop` hook is refused: it would read a released resource, or need a second holder of it whose hook runs out of program order. A use before the write is accepted, and so is a write to a place beside the matched one, such as a sibling field. A binding of a plain value shares the old payload and keeps reading it.

## Messages

- `` `{binding}` is used after {write} writes `{matched}`, the value its arm matched: the write releases the resource `{binding}` reads ``
- `` `{binding}` is handed to {call}, which writes `{matched}`, the value its arm matched: the write releases the resource `{binding}` reads while the call holds it ``

## Help

- `` use `{binding}` before {write}, or write `{matched}` after the match ``
- `` read what the call needs from `{binding}` before it, or write `{matched}` after the match ``

## Before

```miri
class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

enum Pack
    Two(Token, Token)
    Empty
    fn drop(self)
        println("pack dropped")
    fn clear(self)
        self = Pack.Empty

fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            p.clear()
            println(f"{a.id}")
        Pack.Empty: println("none")
    p.drop()
```

## After

```miri
class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

enum Pack
    Two(Token, Token)
    Empty
    fn drop(self)
        println("pack dropped")
    fn clear(self)
        self = Pack.Empty

fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            println(f"{a.id}")
            p.clear()
        Pack.Empty: println("none")
    p.drop()
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
