## Rule

`miri view --around` or `miri patch --old` was given text that does not occur in the function being viewed or patched. The text is matched against the function's canonical rendering, which is the same text `miri view --fn` prints — not against the raw bytes of the file. Comments and the author's spacing are normalized away in that rendering, so anchor on code rather than on layout.

## Before

View:
```sh
miri view --fn main --around "// tally the results" report.mi
```

Patch:
```sh
miri patch --replace-in-fn main --old "// tally the results" --new "result = total" report.mi
```

## After

View:
```sh
miri view --fn main --around "total = total + value" report.mi
```

Patch:
```sh
miri patch --replace-in-fn main --old "total = total + value" --new "result = total" report.mi
```

## Reference

[Build and Command Line](../reference/build.md)
