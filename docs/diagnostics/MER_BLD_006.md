## Rule

`miri view --around` was given text that does not occur in the function being viewed. The text is matched against the function's canonical rendering, which is the same text `miri view --fn` prints — not against the raw bytes of the file. Comments and the author's spacing are normalized away in that rendering, so anchor on code rather than on layout.

## Before

```sh
miri view --fn main --around "// tally the results" report.mi
```

## After

```sh
miri view --fn main --around "total = total + value" report.mi
```

## Reference

[Build and Command Line](../reference/build.md)
