## Rule

A C-style operator has been used instead of the Miri keyword equivalent. Miri does not support operators like `++` (increment) or `--` (decrement). Use the appropriate Miri keyword or function instead (e.g. `+= 1` for increment).

## Before

```miri
let x = &true
```

## After

```miri
let x = true and true
```

## Reference

[Parser](../reference/parser.md)
