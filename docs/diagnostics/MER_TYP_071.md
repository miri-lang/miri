## Rule

A local binding is never read. The value is computed, stored, and then nothing asks for it, so the binding is residue: whatever used to read it is gone. Remove the binding, or read it.

This is the characteristic leftover of an edit. A rename, an extracted function or a deleted branch takes the last reader away and leaves the declaration behind, and nothing about the program stops working — which is why it is reported rather than left silent.

A binding whose name begins with `_` is never reported. That spelling says the value is deliberately not read, and `miri fix` writes it for you: the `underscore-unread-binding` repair renames the binding rather than deleting it, because which of the two answers is right is the author's to pick and only the rename is determined.

Loop variables are not reported: `for index in 0..3` names the iteration whether or not the body reads the name.

## Messages

- `Unused local: '{name}' is never read`

## Before

```miri
fn main()
    let unread = 41
    println("done")
```

## After

```miri
fn main()
    println("done")
```

## Reference

[Type Checker](../reference/types.md)
