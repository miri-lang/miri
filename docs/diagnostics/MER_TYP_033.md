## Rule

This code covers a family of member-access failures. When a field or method name does not exist on a type, this error is raised.

Where the compiler can name a likely intent it does so in the help line. A member name borrowed from another language is matched against the names the receiver actually declares (`len` against `length`, `append` against `push`, `upper` against `to_upper`), and the argument count at the call site breaks ties between members that are otherwise equally close. When the receiver is iterable and the missing member is an accessor such as `keys`, the help points at a `for` loop instead of a member.

## Messages

- `Type '{name}' has no field or method '{member}'`
- `Type '{name}' has no field '{member}'`
- `Type '{name}' does not have members`

## Before

```miri
use system.collections.list

var l = List<int>()
l.push(1)
let n = l.len()
```

## After

```miri
use system.collections.list

var l = List<int>()
l.push(1)
let n = l.length()
```

## Reference

[Type Checker](../reference/types.md)
