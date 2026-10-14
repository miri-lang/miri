## Rule

A value passed to a collection constructor does not match the element type declared for that collection. All elements must be type-compatible with the collection's generic parameter.

## Messages

- `` {collection} must have the same type: the first is `{first}`, but this one is `{found}` ``
- `` {member} cannot be optional, but this one is `{type}` ``
- `` {collection} cannot hold only `None` ``
- `` Invalid map key type: the map is keyed by `{key}`, but this key is `{found}` ``

## Help

- `` write every element as `{type}`, or declare the collection's element type ``
- `` convert this element with `as {type}` ``
- `` store `{type}` and leave the absent values out of the set ``
- `` key the map by `{type}` and leave the absent keys out ``
- `` `None` alone has no element type; build an empty one instead, e.g. `{example}` ``
- `` convert the value with `as {type}` ``
- `` use a value of type `{expected}` here, or change the type this place declares to `{found}` ``

## Before

```miri
fn main() i32:
  let list = [1, 2, "three"]
  0
```

## After

```miri
fn main() i32:
  let list = [1, 2, 3]
  0
```

## Reference

[Type Checker](../reference/types.md)
