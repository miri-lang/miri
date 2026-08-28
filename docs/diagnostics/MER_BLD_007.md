## Rule

`miri view --around` or `miri patch --old` was given text that occurs more than once in the function being viewed or patched, so the target is ambiguous. Extend the anchor until it identifies one site only: include the surrounding line, or a neighbouring statement, rather than a fragment that repeats.

## Before

View:
```sh
miri view --fn main --around "index" walk.mi
```

Patch:
```sh
miri patch --replace-in-fn main --old "index" --new "idx" walk.mi
```

## After

View:
```sh
miri view --fn main --around "index = index + 1" walk.mi
```

Patch:
```sh
miri patch --replace-in-fn main --old "index = index + 1" --new "idx = idx + 1" walk.mi
```

## Reference

[Build and Command Line](../reference/build.md)
