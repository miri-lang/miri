## Rule

A resource type (a class with a `fn drop(self)` method) was declared in a scope but was never consumed before the scope exited. The drop method will still execute via reference counting, but the code pattern suggests the resource may have been forgotten. Explicitly consume the resource to silence the warning.

## Messages

- `resource '{var}' of type '{type}' was not consumed before scope exit`

## Before

```miri
class File
    fn drop(self)
        println("Closing file")

let f = File()
```

## After

```miri
class File
    fn drop(self)
        println("Closing file")

let f = File()
f.drop()
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
