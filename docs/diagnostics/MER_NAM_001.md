## Rule

The name `gpu` (lowercase) is deprecated and will be removed in a future release. Use `kernel` instead. Both refer to GPU kernel code context, but the new name aligns with Miri's terminology.

## Messages

- `` `{old}` is deprecated; use `{new}` instead ``

## Before

```miri
gpu fn process()
    let result = gpu_context
    return
```

## After

```miri
gpu fn process()
    return
```

## Reference

[Naming and Identifiers](../reference/naming.md)
