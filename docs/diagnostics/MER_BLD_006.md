## Rule

`miri view --around` or `miri patch --old` was given text that does not occur in the function being viewed or patched. The text is matched against the function's canonical rendering, which is the same text `miri view --fn` prints — not against the raw bytes of the file. Comments and the author's spacing are normalized away in that rendering, so anchor on code rather than on layout.

## Messages

- `` `{anchor}` does not occur in this function ``

## Help

- `the anchor is matched against canonical source, where comments and original spacing are normalized away`

## Before

`report.mi`:

```miri
fn main()
    var total = 0
    // tally the results
    for value in [1, 2, 3]
        total = total + value
    println(f"{total}")
```

```sh
miri view --fn main --around "// tally the results" report.mi
miri patch --replace-in-fn main --old "// tally the results" --new "result = total" report.mi
```

## After

```sh
miri view --fn main --around "total = total + value" report.mi
miri patch --replace-in-fn main --old "total = total + value" --new "total = total + value * 2" report.mi
```

## Reference

[Build and Command Line](../reference/build.md)
