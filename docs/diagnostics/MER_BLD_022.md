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

## Before

```sh
# The name is misspelled, or its module is not imported.
miri view app.mi --type Circel
# error[MER_BLD_022]: Type Not In Scope
#   = help: did you mean 'Circle'?
```

## After

```sh
# Ask about a name the program has.
miri view app.mi --type Circle

# Or list what a module declares, to find the name it is spelled with.
miri view system.collections.list --outline --public
```

## Reference

[Type Checker](../reference/types.md)
