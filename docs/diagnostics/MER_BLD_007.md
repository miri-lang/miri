## Rule

`miri view --around` was given text that occurs more than once in the function being viewed, so the block to show is ambiguous. Extend the anchor until it identifies one site: include the surrounding line, or a neighbouring statement, rather than a fragment that repeats.

## Before

```sh
miri view --fn main --around "index" walk.mi
```

## After

```sh
miri view --fn main --around "index = index + 1" walk.mi
```

## Reference

[Build and Command Line](../reference/build.md)
