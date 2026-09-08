## Rule

`miri patch` failed to align the canonical (normalized) rendering of the function to the raw source. This happens when the source contains constructs whose canonical rendering differs from the source text, such as redundant parentheses (`((expr))` renders to `(expr)`). The file was left unchanged, and the patch was not applied.

A number is not one of them: the rendering the alignment is built against reads a number's spelling out of the source, so `0xFF` and `1_000` align as themselves.

To fix this, reformat the source to match the canonical form — `miri fmt` does it — and the refusal names the declaration it was reading, so a file with several of them says which one to rewrite.

The same code reports an anchor that begins or ends partway through a token. `--old "v={a}"` inside `f"v={a}"` names part of a literal, and the tokens it covers whole are a different stretch of text than the one asked for; anchor on the whole literal instead. A declaration that cannot be anchored is still a landmark: `--insert-fn ... --after` places a new declaration beside it, because an insertion beside a declaration only has to know where it ends.

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
