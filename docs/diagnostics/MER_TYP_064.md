## Rule

A declaration reuses a name in a way shadowing does not allow. A `let` may shadow an earlier `let` in the same scope, but a `var` may not shadow anything in its scope, a `let` may not shadow a `var` in the same scope, and no declaration may shadow a constant or declare a constant over an existing name. To change a variable's value, assign to it.

## Messages

- `Variable '{name}' is already defined in this scope. 'var' cannot shadow existing variables.`
- `Cannot shadow mutable variable '{name}' with an immutable one in the same scope.`
- `Cannot shadow constant '{name}'.`
- `Cannot shadow existing variable/constant '{name}' with a constant.`

## Before

```miri
fn main()
    var total = 5
    var total = 10
    println(f"{total}")
```

## After

```miri
fn main()
    var total = 5
    total = 10
    println(f"{total}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
