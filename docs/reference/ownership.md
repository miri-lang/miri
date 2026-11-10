# Ownership and Resource Management

Miri manages memory with reference counting and inferred borrowing over copy-on-write values. Every struct, tuple, collection and class instance is a value: a second binding shares it until either writes, and the writer copies storage that is still shared, so a write through one name is never seen through another. A class or enum with a `drop` hook is a resource: one owner, moved on assignment, its hook run exactly once. Nothing is a reference. A call lends its arguments, and `out`, declared on the parameter and spelled at the call, is the only way a callee assigns to a caller's value.

## What It Rejects

- Resource variables not consumed before scope exit (warning; the drop method still runs)
- Linear variables not consumed exactly once (error)
- Use-after-move: accessing a consumed variable (error)
- Calling `drop()` on a value the scope does not own — a parameter, `self`, a field, a loop or match binding, or a captured variable (error)
- A class or trait `drop` that takes arguments or is static (error): the name belongs to the drop hook
- Discarding values that must be used (types marked `@must_use`)
- A store that puts an object back into one of its own fields — the object itself, a closure capturing it, or a value holding it (error, `MER_TYP_081`): reference counting never frees a cycle
- A write to a value a closure or nested function captured (error, `MER_TYP_090`): a closure holds its own copy, so the write would be lost when the call ends

## Key Concepts

- **Three kinds, decided by the type alone**: a value (structs, tuples, collections, strings, class instances without a hook), a resource (a class or enum whose release may run a `drop` hook, and anything containing one), and no reference. What must be reached from many places and changed through any of them is a handle into an arena one owner holds
- **Bitwise values**: A value with only fixed-size, bitwise fields is plain bytes, copied on assignment
- **A call lends**: The callee reads its arguments and the caller keeps them. Whole-program inference decides which callees keep an argument; only a resource argument the callee keeps is consumed by the call, and a call through a function value the compiler cannot see into counts as keeping it. The rule is the same at the top level of a file as inside a function
- **`out`**: Declared on the parameter (`fn bump(n out int)`) and spelled at the call (`bump(out n)`). It names a place rooted at a `var` (the variable, a field, an element, an optional payload bound `Some(var cur)`), written through at most one `out` path per call; no other operand of the call may lend a value containing that place, and an operand naming a sibling sees the value from before the call. The place is taken for the call and restored on every exit
- **Methods**: A method may be called on any binding, `let` included. A method that writes its receiver, called on a lent parameter, writes the caller's value, which is taken for the call and restored after it
- **Loop bindings**: `for e in es` binds the element `es[i]`; assigning through `e` needs `es` writable, calling a method on `e` does not
- **Release**: A hook-free value is released at its last use; a resource where it is consumed, else at its owner's scope end in reverse declaration order. `List` elements are released in index order
- **Drop hook**: A class declares its hook as `fn drop(self)` (or `fn drop()`, since a class method's receiver is implicit), inherits one from its base class, or takes a default `drop` from a trait it implements. Only classes and enums have one: a struct is data only and declares no methods, so no drop hook
- **Calling drop**: `value.drop()` on a local the scope declared, or on a call result, runs the hook at that point and consumes the value; the hook does not run again when the scope ends
- **Linear variables**: Universally unique; cannot be duplicated or dropped without use
- **No cycles**: Nothing is a reference, so a write that would close a cycle writes a copy instead, and every value graph is acyclic

## Per-Code Detail

Use `miri explain MER_OWN_<code>` for detailed guidance on each ownership diagnostic code.
