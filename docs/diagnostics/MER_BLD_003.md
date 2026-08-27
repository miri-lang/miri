## Rule

The `miri determinism check` command verifies that a program compiles to byte-identical artifacts across multiple builds. Non-determinism in build artifacts indicates that compiler decisions (iteration order, hash-based choices, or randomness) are affecting the output, rather than solely depending on the source code.

## Before

```
miri determinism check main.mi
# error: bytes differ at offset 1848
#   run 1: 1b 00 00 00 18 00 00 00 01 e6 71 02
#   run 2: 1b 00 00 00 18 00 00 00 bb 5a 32 4d
# MER_BLD_003: non-deterministic artifact
```

## After

Determinism is typically affected by:
1. **Unordered iteration**: Collections like `HashMap` are iterated in an unordered manner across runs. Convert them to ordered types like `BTreeMap`.
2. **Randomness in compilation**: Ensure compiler passes use deterministic algorithms (no random seeds, stable sort order).
3. **Time-dependent values**: Ensure build metadata (timestamps, process IDs) is not embedded in artifacts.

Investigate the bytes at the reported offset to determine whether the divergence stems from unordered iteration (collection ordering), random data, or time-dependent values.

```
# Re-run to confirm it's non-deterministic, not a transient I/O error
miri determinism check main.mi
```

## Reference

[Build and Command Line](../reference/build.md)
