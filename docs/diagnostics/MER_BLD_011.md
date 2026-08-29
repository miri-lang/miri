## Rule

`miri patch` applied an edit to the source code, but re-validation of the resulting file revealed compiler errors that the edit introduced. The patch was rejected and the file was left unchanged, protecting against silently introducing errors. The envelope carries both the new errors (errors the edit caused) and any pre-existing errors (errors that existed before the edit). Only new errors cause rejection; an edit that leaves pre-existing errors untouched is accepted and written, with those pre-existing errors reported.

To proceed, fix the issues identified in the errors and try the patch again. Alternatively, use `--check-only` to validate without writing.

## Before

Source file with an existing error:
```miri
fn add(a int, b int) int
    return a + b

fn broken() int
    return "text"
```

Patch that introduces a new error:
```sh
miri patch --replace-in-fn add --old "a + b" --new "a + true" code.mi
```

This is rejected because the edit introduces a new type error.

## After

Either fix the patch to avoid introducing new errors:

```sh
miri patch --replace-in-fn add --old "a + b" --new "a + 1" code.mi
```

Or use `--check-only` to see what errors would result:

```sh
miri patch --replace-in-fn add --old "a + b" --new "a + true" --check-only code.mi
```

Note: An edit that leaves pre-existing errors untouched is accepted and written:

```sh
miri patch --replace-in-fn add --old "a + b" --new "a - b" code.mi
```

This succeeds because the edit does not introduce new errors, even though the file still has the pre-existing error in `broken()`.

## Reference

[Build and Command Line](../reference/build.md)
