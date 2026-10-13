## Rule

`miri view --around` or `miri patch --old` was given text that occurs more than once in the function being viewed or patched, so the target is ambiguous. Extend the anchor until it identifies one site only: include the surrounding line, or a neighbouring statement, rather than a fragment that repeats.

## Messages

- `` `{anchor}` occurs {count} times in this function ``

## Help

- `extend the anchor until it matches one site only`

## Before

`walk.mi`:

```miri
fn main()
    var index = 0
    while index < 3
        index = index + 1
    println(f"{index}")
```

```sh
miri view --fn main --around "index" walk.mi
miri patch --replace-in-fn main --old "index" --new "idx" walk.mi
```

## After

```sh
miri view --fn main --around "index = index + 1" walk.mi
miri patch --replace-in-fn main --old "index = index + 1" --new "index = index + 2" walk.mi
```

## Reference

[Build and Command Line](../reference/build.md)
