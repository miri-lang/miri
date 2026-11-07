## Rule

Two top-level functions in one file share a name, or two methods of one
class, enum or trait do.

A call resolves to one declaration, and nothing in the language says which. The
second declaration used to overwrite the first in the symbol table, so the file
type-checked clean and failed later in code generation, where the backend
refuses to define one symbol twice — a failure carrying no source position and
no way for a caller to know which declaration to remove.

The check runs over the file's own declarations, so a name declared here and
also imported from a module is a different diagnostic; importing a name that
collides is reported where the import is written.

Miri has no overloading: two functions of the same name are a redeclaration
whatever their parameters. The same holds for the methods of one type: a
second method of a name is refused where it is written, with a note pointing
at the first, which is the one kept. A subclass declaring a method of the
class it extends overrides it, and a class declaring a method of a trait it
implements supplies it; neither is a redeclaration. A static and an instance
method of one name are refused as well, under `MER_TYP_055`.

## Messages

- `Function '{name}' is already declared in this file`
- `Method '{name}' is already declared in this {kind}`

## Help

- `an earlier declaration of '{name}' appears above; rename one of them or remove this declaration.`
- `Miri has no overloading: a {kind} declares each method name once; rename one of them or remove this declaration.`

## Before

```miri
fn f() int
    1

fn f() int
    2

fn main()
    println(f"{f()}")
```

## After

```miri
fn f() int
    1

fn g() int
    2

fn main()
    println(f"{f()} {g()}")
```

## Reference

[Type Checker](../reference/types.md)
