## Rule

A declaration marked `private` is never used in the file that declares it. `private` says the name is reachable from nowhere else, so a file that does not use it has a declaration nothing can ever reach.

Remove it, or make it `public` if it is meant to be part of the module's surface. Marking it `public` is the spelling that turns this report off for a declaration that is deliberately kept.

A public declaration is never reported. An exported name is used by definition — by whoever imports the module — and this check looks at one file, so it cannot see those callers.

## Messages

- `Unused private declaration: '{name}' is never used in this file`

## Help

- `remove the {noun}, or declare it 'public' if it is meant to be part of this module's surface.`

## Before

```miri
private fn orphan() int
    41

fn main()
    println("done")
```

## After

```miri
fn main()
    println("done")
```

## Reference

[Type Checker](../reference/types.md)
