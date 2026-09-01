# MER_BLD_018: Formatter Not Idempotent

## Rule

The formatter must produce a fixed point: canonical text parsed and re-rendered produces the same text.

## Before

```miri
fn example()
    let x = 1
```

## After

```miri
fn example()
    let x = 1
```

## Reference

[Build and Command Line](../reference/build.md)
