## Rule

Calling a resource's drop hook directly, as `value.drop()`, runs the hook and releases the value at that point, so it is allowed only on a value the current scope owns: a `let` or `var` local, or the result of a call. A parameter is still held by the caller, `self` by whoever called the method, a field by its object, a loop or match binding by its collection or subject, and a captured variable by the scope that declared it. Releasing any of those here would run the hook a second time when the real owner lets go, so the call is refused. Drop the value where it is owned, or pass it to a function and let its owner release it.

## Messages

- `drop() can only release a value this scope owns`

## Help

- `call drop() on the local variable that owns the value, or let its owner release it`

## Before

```miri
class Handle
    public var id int

    fn drop(self)
        println(f"closed {self.id}")

fn close(h Handle)
    h.drop()

let h = Handle(id: 1)
close(h)
```

## After

```miri
class Handle
    public var id int

    fn drop(self)
        println(f"closed {self.id}")

let h = Handle(id: 1)
h.drop()
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
