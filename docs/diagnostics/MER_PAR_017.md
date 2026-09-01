## Rule

Two or more access modifiers or other declarative modifiers have been combined in a way that is invalid. For example, a declaration cannot be both `public` and `private`, or both `public` and `shared`. Check for conflicting modifiers in the same declaration.

## Messages

- `Invalid Modifier Combination`

## Before

```miri
async gpu fn kernel()
    return
```

## After

```miri
gpu fn kernel()
    return
```

## Reference

[Parser](../reference/parser.md)
