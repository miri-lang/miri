## Rule

A builtin type constructor (e.g., `i32()`, `List<T>()`) was called with invalid arguments. The constructor's arity or argument types do not match what the builtin type expects, or the constructor was used on a type that does not support construction.

## Messages

- `List<{element}>(...) expects an array or list of '{element}', got '{got}'. Use 'List<{element}>()' for an empty list`
- `List(...) expects an array or list argument, got '{got}'. Use 'List<T>()' for an empty list or 'List([...])' to convert an array`
- `Constructor 'init' in class '{class}' must call super.init() because parent class '{parent}' has a constructor`

## Before

```miri
use system.collections.list

fn main()
    let names = List<String>("Ada")
    println(f"{names.length()}")
```

## After

```miri
use system.collections.list

fn main()
    let names = List<String>(["Ada"])
    println(f"{names.length()}")
```

## Reference

[Type Checker](../reference/types.md)
