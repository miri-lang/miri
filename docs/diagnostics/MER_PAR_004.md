## Rule

The left-hand side of an assignment is not a valid lvalue. An assignment target must be an identifier, object member, or collection subscript that can receive a new value. Literals, function calls, or other expressions that do not refer to a storage location cannot be assigned to.

## Messages

- `Invalid Left-Hand Side Expression`

## Before

```miri
fn main()
    5 = 10
    (x + y) = 20
    get_value() = 30
```

## After

```miri
fn main()
    let x = 5
    let y = 10
    var z = get_value()
    z = 30
```

## Reference

[Parser](../reference/parser.md)
