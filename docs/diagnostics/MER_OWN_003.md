## Rule

A value was consumed and then used again. For resource types (classes with `fn drop(self)`), assignment is a move: the source variable is transferred to the destination and cannot be accessed afterward. For other types, the same rule applies at top-level scope. Call `.clone()` to keep an independent copy.

## Before

```miri
class Resource
    fn drop(self)
        return

fn sink(x Resource)
    return

var r = Resource()
sink(r)
sink(r)
```

## After

```miri
class Resource
    fn drop(self)
        return

fn sink(x Resource)
    return

var r = Resource()
sink(r)
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
