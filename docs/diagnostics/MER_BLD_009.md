## Rule

`miri patch --expect-sha` was given a SHA-256 hash that does not match the file's current hash. The file has changed since the hash was recorded or calculated. This guard prevents applying a stale patch to a file that has been modified. Re-run without `--expect-sha` to proceed regardless, or re-calculate the hash of the current file.

## Messages

- `the file now hashes to {actual}, and the edit was prepared against {expected}`

## Help

- `read the file again, rebuild the edit against what it says now, and retry`

## Before

`source.mi`:

```miri
fn main()
    let x = 1
    let y = x + 1
    println(f"{y}")
```

```sh
miri patch --replace-in-fn main --old "x + 1" --new "x * 2" \
  --expect-sha "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" \
  source.mi
```

The hash names a version of `source.mi` that is not the one on disk.

## After

Calculate the current SHA-256 hash and use it:

```sh
SHA=$(sha256sum source.mi | cut -d' ' -f1)
miri patch --replace-in-fn main --old "x + 1" --new "x * 2" \
  --expect-sha "$SHA" \
  source.mi
```

Or proceed without the hash guard:

```sh
miri patch --replace-in-fn main --old "x + 1" --new "x * 2" source.mi
```

## Reference

[Build and Command Line](../reference/build.md)
