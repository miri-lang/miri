## Rule

`miri view --type <NAME>` was asked about a name the program does not have in
scope.

The list this reads is the type table the frontend built, so a type counts as in
scope when the program could name it: its own declarations, whatever its `use`
statements bring in, and the implicit prelude. A library type that no `use`
statement imports is not in scope, even though the file declaring it exists.

With no path, the question is asked against an empty program, whose scope is the
prelude alone. That is what lets a caller ask about a library type before
knowing which module declares it, and it is also why a type from a module
outside the prelude needs a file that imports it.

## Messages

- `no type named '{name}' is in scope`

## Help

- `did you mean '{name}'?`
- `` '{name}' is declared in {module}, which this file does not import: add `use {module}`, or run `miri view {module} --type {name}`. ``

## Before

`app.mi`:

```miri
class Circle
    var radius float
```

```sh
miri view app.mi --type Circel
```

The name is misspelled, or its module is not imported.

## After

```sh
# Ask about a name the program has.
miri view app.mi --type Circle

# Or list what a module declares, to find the name it is spelled with.
miri view system.collections.list --outline --public
```

## Reference

[Type Checker](../reference/types.md)
