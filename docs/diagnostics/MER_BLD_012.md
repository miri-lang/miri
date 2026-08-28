## Rule

`miri patch` was given a set of edit flags that do not describe a coherent edit. Each edit names one function and one replacement, so `--replace-in-fn`, the text to find, and the text to put in its place must arrive in equal numbers and pair up in the order they are written. The same holds for `--replace-fn` and `--body-file`.

Two further rules keep a batch unambiguous. The text to find comes either from `--old` or from `--old-file`, not from both in one call, and the same for `--new`; mixing the two leaves no way to say which of them a given edit meant. And because there is only one standard input, at most one file argument in a call may be `-`.

## Before

```sh
miri patch --replace-in-fn total --old "sum + 1" file.mi
```

## After

```sh
miri patch --replace-in-fn total --old "sum + 1" --new "sum + 2" file.mi
```

## Reference

[Build and Command Line](../reference/build.md)
