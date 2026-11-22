## Rule

A value whose release runs a drop hook gained a second holder. A class or
enum with a `fn drop(self)` hook is a resource: it has one owner, assigning it
moves it, and its hook runs exactly once, when that owner lets go. Its count
is therefore never more than one. Under `MIRI_HOOK_CHECK=1`, in a build
without `--release`, every block of such a type is marked when it is built,
and an increment that raises a marked block past one stops the program here,
because the hook would otherwise run when the last holder lets go rather than
when the owner does.

Ordinary Miri code cannot do this: the compiler moves a resource rather than
sharing it. The trap points at a compiler defect in that ownership, or a test
helper that shares a hooked block on purpose. Without the check the same
fault goes unreported and the hook runs late.

## Messages

- `Hooked Value Shared`
- `a value whose release runs a drop hook gained a second holder (count {count}, raised by {who}), so its hook would run when the last holder lets go rather than when its owner does; this is a compiler defect`

## Before

```miri
use system.testing

fn main()
    simulate_shared_hooked_block()
```

## After

```miri
class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

fn main()
    let t = Token(id: 1)
    let moved = t
    println(f"{moved.id}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
