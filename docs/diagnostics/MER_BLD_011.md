## Rule

`miri patch` applied an edit to the source code, but re-validation of the resulting file revealed compiler errors. Because the file no longer type-checks, the patch was rejected and the file was left unchanged. This protects against silently introducing errors. The envelope carries the real type errors that were found; this code explicitly marks that the file was not written.

To proceed, fix the issues identified in the type errors and try the patch again, or use `--check-only` to validate without writing.

## Before

Source file (valid code):
```miri
fn add(a int, b int) int
    return a + b
```

Patch that would break type checking:
```sh
miri patch --replace-in-fn add --old "a + b" --new "a + true" code.mi
```

## After

Either fix the patch to preserve type correctness:

```sh
miri patch --replace-in-fn add --old "a + b" --new "a + 1" code.mi
```

Or use `--check-only` to see what the type errors would be without writing:

```sh
miri patch --replace-in-fn add --old "a + b" --new "a + true" --check-only code.mi
```

## Reference

[Build and Command Line](../reference/build.md)
