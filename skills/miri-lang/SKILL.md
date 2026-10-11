---
name: miri-lang
description: Writing Miri source files — syntax, type system, and iteration workflow
---

# Miri Language Essentials

The short core for writing `.mi` files: how Miri differs from Python and Rust,
the traps that cost earlier agents the most, and the one check → fix → test
loop. Everything else is a lookup, not a read: ask the compiler (see *Looking
things up*) rather than guessing a name.

## Core Syntax

Indentation-sensitive blocks; a `:` puts a one-line body on the header line.

```miri
use system.collections.list
use system.collections.map

struct Point
    x int
    y int

enum Color
    Red
    Blue

trait Drawable
    fn draw()

class Circle implements Drawable
    fn draw(): println("circle")

fn add(a int, b int) int
    a + b

fn main()
    let x int = 5
    var y = 10
    y = add(x, y)
    let p = Point(3, 4)
    var words = Map<String, int>()
    words["a"] = 1
    let items = List([1, 2, 3])
    for item in items
        if item > 1 and not (item == 3): println(f"{item} {p.x}")
    match Color.Red
        Color.Red: println("red")
        Color.Blue: println("blue")
    match Result.Ok(y)
        Result.Ok(v): println(f"ok {v}")
        Result.Err: println("err")
    let maybe int? = words.get("a")
    match maybe
        Some(v) if v > 0: println(f"some {v}")
        Some(_) | None: println("none")
```

## Where Miri Differs

- Bindings are `let` (immutable) and `var` (mutable); there is no `let mut`.
  Parameters are immutable too; mutate a collection argument with `.set(i, v)`.
- Types follow the name with a space: `let x int = 5`, `fn f(a int) int`. No `:`
  before a type, no `->`, no literal suffixes (`42i32`); cast with `as`.
- Logic is `and`, `or`, `not`. Nullable is `T?` with bare `Some`/`None`;
  `Result` patterns need the prefix: `Result.Ok(v)`, `Result.Err`.
- `List([1, 2])` builds from a literal, `List<int>()` is empty, `Map<K, V>()` and
  `Set<T>()` likewise. `[1, 2, 3]` alone is a fixed `Array`; `{}` is a `Map`.
- A `for` over a `Map` walks its keys; there is no `.keys()`.
- An f-string is the only int → String conversion: `f"{n}"`, never `n.to_string()`
  or `"n=" + n`.
- `main() int` returns an exit status; `panic(message)` stops with status 1.
  There is no `exit`.
- Stdlib modules are imported by name (`use system.collections.list`).

```miri,fails=MER_PAR_001,expects-message=Expected an expression, but found :
let x: int = 5
```

```miri,fails=MER_TYP_038,expects-message=Expected enum variant pattern like Result.Ok
fn main()
    match Result.Ok(5)
        Ok(x): x
        Err: 0
```

```miri,fails=MER_TYP_036,expects-message=Cannot instantiate generic class 'List<T>' without explicit type arguments
use system.collections.list

fn main()
    let l = List()
```

## Traps

- **Quotes inside f-string braces** must be the other quote character:
  `f"{m['a']}"`. The same quote ends the string (MER_LEX_012).
- **`length()` counts characters; `substring(start, end)` takes byte offsets.**
  On non-ASCII text they disagree silently; check with a test that uses one.
- **Integer overflow wraps silently** at the declared width (`int` is 64-bit).
  Guard a sum that can exceed it before you add.
- **Walk with `for`, not by position.** `for ch in s` and `for k in m` are one
  pass; indexing a `String`, `Map` or `Set` by a counter repeats lookups.
- **Imports are explicit.** A name from another module needs its `use` line;
  `miri fix main.mi --apply --yes` adds one that resolves in exactly one module.

## Looking things up

Ask before guessing a member, a module or a signature. These are cheap and exact:

- `miri view --type String --public` lists what can be called on a type,
  inherited and trait-supplied members included (`List`, `Map`, `int` alike).
- `miri view system.io --outline --public` lists what a module offers; try
  `system.fs`, `system.os`, `system.collections.map` the same way.
- `miri view system.fs --fn read_file` prints one function.
- `miri view --stdlib-root` prints the stdlib roots searched; set
  `MIRI_STDLIB_PATH` when the binary is not beside its `stdlib/`.
- `miri explain MER_TYP_033` describes the rule behind a diagnostic code.

The `miri-testing` pack covers `miri test`; the `miri-gpu` pack covers kernels.

## Verification Loop

This is the one loop the compiler's documentation recommends; everything else defers to it. **Loop:** `miri check --format json` → edit → `miri test` (or `miri run`). Iterate until clean.

1. **miri check** — `miri check myfile.mi --format json`. Each diagnostic has a `code`, a `help` and, where one exists, a `repair` with its edits. A diagnostic without a `repair` is one you edit yourself.
2. **Edit** — `miri fix myfile.mi --apply --yes` writes every recorded repair in one call. Everything else is an ordinary edit; in a file already in canonical layout, `miri patch myfile.mi --replace-in-fn name --old text --new text` makes a scoped edit and re-checks before anything reaches disk.
3. **miri test** / **miri run** — `miri test --dir <DIR>`, `miri run myfile.mi`; both take `--format json`. Only a run finds a fault the frontend cannot see.

The auto-applicable repairs are:
- `add-import`: Import a name that resolves in exactly one module.
- `arrow-return-type`: Drop the `->` before a return type.
- `colon-annotation`: Drop the `:` before a type annotation.
- `concat-to-formatted-string`: Rewrite a `+` chain joining text to values as one f-string.
- `drop-extra-arguments`: Drop positional arguments a call does not declare.
- `drop-iterator-accessor`: Drop a `keys` accessor on a keyed collection and iterate it directly.
- `if-colon`: Write `:` in place of `then`, or after an inline `else`.
- `interpolation-quotes`: Write the string literals inside an f-string's braces with the other quote.
- `let-mut-to-var`: Rewrite a `let mut` binding as `var`.
- `let-to-var`: Rebind an immutable declaration as mutable.
- `null-to-none`: Rewrite `null`, `nil` or `nullptr` as `None`.
- `println-bang`: Drop the `!` from a macro-style call.
- `qualify-variant-pattern`: Prefix a bare variant pattern with the enum that declares it.
- `rename-to-suggestion`: Rewrite a name as the one spelling the help offers in its place.
- `underscore-unread-binding`: Prefix a binding nothing reads with `_` to say the value is not meant to be read.
- `word-operator`: Rewrite `!`, `&&` or `||` as `not`, `and` or `or`.
