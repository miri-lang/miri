## Rule

`miri patch` failed to align the canonical (normalized) rendering of the function to the raw source. This happens when the source contains constructs whose canonical rendering differs from the source text, such as redundant parentheses (`((expr))` renders to `(expr)`) or non-canonical numeric literals (`1.50` renders to `1.5`). The file was left unchanged, and the patch was not applied.

To fix this, reformat the source to match the canonical form: remove extra parentheses, and use canonical numeric literals (e.g., `1.5` instead of `1.50`).

## Before

Source file:
```miri
fn demo() int
    return ((1 + 1))
```

Patch attempt:
```sh
miri patch --replace-in-fn demo --old "1 + 1" --new "2 + 2" code.mi
```

## After

Reformat the source to remove redundant parentheses:

```miri
fn demo() int
    return 1 + 1
```

Then apply the patch:

```sh
miri patch --replace-in-fn demo --old "1 + 1" --new "2 + 2" code.mi
```

## Reference

[Build and Command Line](../reference/build.md)
