---
name: miri-lang
description: Writing Miri source files — syntax, type system, and iteration workflow
---

# Miri Language Essentials

This skill teaches the core syntax and idioms for writing `.mi` source files. It covers what the language supports, what it explicitly does not support, and the workflow for arriving at correct code.

## Positive Grammar

Miri's syntax is indentation-sensitive. Variables use space-separated type annotations (not colon form). Here is the core:

```miri
use system.collections.list

struct Point
    x int
    y int

enum Color
    Red
    Blue

fn add(a int, b int) int
    a + b

fn demo_color(color Color)
    match color
        Color.Red: println("red")
        Color.Blue: println("blue")

class Shape
    fn area() int: 0

trait Drawable
    fn draw()

class Circle implements Drawable
    fn draw(): println("circle")

fn identity<T>(x T) T
    x

fn main()
    let x int = 5
    var y int = 10
    y = 20
    let p = Point(3, 4)
    let sum = add(x, y)
    let items = List([1, 2, 3])
    println(f"{sum} {p.x} {items.length()} {identity(7)}")
    demo_color(Color.Red)
    match Result.Ok(5)
        Result.Ok(v): println(f"ok {v}")
        Result.Err: println("err")
    let maybe int? = None
    match maybe
        Some(v): println(f"some {v}")
        None: println("none")
```

This core powers all Miri programs. The rest of this skill covers what the language **does not** support, because those gaps matter just as much as the syntax that works.


## Anti-Hallucination: Syntax That Does Not Exist

Miri deliberately leaves these off the surface to avoid ambiguity. Every example below is a compile error. Before guessing a member name, ask: `miri view --type String --public` lists what can be called on a type, including what it inherits and what a trait supplies, and needs no path for a prelude type.

### Type Annotations on Bindings

The colon form does not exist. Use space-separated type annotations instead:

```miri,fails=MER_PAR_001,expects-message=Expected an expression, but found :
let x: int = 5
```

Write the type after the name, and cast when it cannot be inferred:

```miri
let x int = 5
let z = 5 as i64
```

### Literal Type Suffixes

No suffix syntax like `1i32` or `3.14f64`. Miri infers numeric width from
context; cast when you need a particular one.

```miri,fails=MER_TYP_034,expects-message=Undefined variable: i32
let x = 42i32
```

### Result and Option Match Arms: The Asymmetry

**Result** requires the constructor prefix (`Result.Ok`, `Result.Err`). Omitting it causes a type error:

```miri,fails=MER_TYP_038,expects-message=Expected enum variant pattern like Result.Ok
fn main()
    let r = Result.Ok(5)
    match r
        Ok(x): x
        Err: 0
```

Correct form uses the prefix:

```miri
fn main()
    let r = Result.Ok(5)
    match r
        Result.Ok(x): x
        Result.Err: 0
```

**Nullability (`T?`)** uses bare `Some` and `None` without a prefix:

```miri
fn main()
    let x int? = 5
    match x
        Some(v): v
        None: 0
```

The key difference: `Result` is a standalone type that requires its constructor name in patterns; `T?` is syntactic sugar for nullability where `Some`/`None` are bare constructors.

### Match Arm Bodies: One Line or an Indented Block

An arm header may end with a colon or not, and its body may sit on that line or
indented below it. All four spellings mean the same thing. An indented body may
hold several statements; a same-line body is one expression. Alternative
patterns are separated by `|`, and an arm may carry an `if` guard:

```miri
fn weigh(n int) int
    var weight = 0
    match n
        m if m > 10:
            weight = weight + 10
            weight = weight + 5
        0 | 1: weight = weight + 1
        _
            weight = weight + 2
    weight

fn main()
    println(f"{weigh(50)} {weigh(0)} {weigh(5)}")
```

### Collection Constructor Forms

The three forms are distinct and mean different things:

- `List([1, 2, 3])` — allocate and populate from a literal
- `List<int>()` — allocate empty with explicit type argument
- `{}` — an empty `Map<void, void>` (not a Set or List)

Confusing them is the most common error:

```miri,fails=MER_TYP_036,expects-message=Cannot instantiate generic class 'List<T>' without explicit type arguments
use system.collections.list

fn main()
    let l = List()  // Missing generic type argument
```

Correct forms:

```miri
use system.collections.list
use system.collections.map
use system.collections.set

fn main()
    let l = List([1, 2, 3])
    let empty = List<int>()
    let m = Map<String, int>()
    let s = Set<int>()
```

### No `let mut` — Use `var` Instead

The keyword `let mut` does not exist:

```miri,fails=MER_PAR_001,expects-message=Expected an end of statement, but found identifier
let mut x = 5
```

Use `var`:

```miri
var x = 5
x = 10
```

### Parameters Are Immutable

Function parameters are immutable, even if the argument is mutable. To mutate the caller's variable, the method must exist in the stdlib or be written as a free function accepting a mutable reference pattern (not yet available). The closest common case:

```miri,fails=MER_TYP_042,expects-message=Cannot assign to element of immutable variable
use system.collections.list

fn increment(arr [int]):
    arr[0] = arr[0] + 1

fn main():
    var a = List([5, 10])
    increment(a)
```

Methods that mutate use `.set()`:

```miri
use system.collections.list

fn increment(arr [int]):
    arr.set(0, arr[0] + 1)

fn main():
    var a = List([5, 10])
    increment(a)
```

## Module Resolution

Bare imports are searched in this order: stdlib (via `MIRI_STDLIB_PATH` env var), the entry file's directory, then the current working directory. Local imports (`use local.*`) resolve only against the entry file's directory. The stdlib cannot be shadowed; `system` is reserved. Set `MIRI_STDLIB_PATH` to override the stdlib location when the binary is not beside `stdlib/`; `miri view --stdlib-root` prints the roots actually searched and whether each exists. `miri view` accepts a module name wherever it accepts a path, so `miri view system.io --outline --public` lists what a module offers without knowing where it lives on disk.

## Iteration and Control Flow

**For loops over collections and ranges:**

```miri
use system.collections.list

fn main()
    let xs = List([1, 2, 3])
    for x in xs
        println(f"{x}")
    for n in 0..3
        println(f"n {n}")
    var i = 0
    while i < 3
        i = i + 1
```

**Early return and if/else chains:**

```miri
fn classify(n int) String
    if n < 0
        return "negative"
    else if n == 0
        return "zero"
    "positive"

fn main()
    println(classify(-1))
    println(classify(0))
    println(classify(5))
```

## String Conversion and Collections

**f-strings are the only int→String conversion path:**

```miri
fn main()
    let n = 42
    let s = f"{n}"
    println("count: " + s)
    println(f"n={n} half={n / 2}")
```

Attempting other conversion methods fails:

```miri,fails=MER_TYP_002,expects-message=cannot add String and int
fn main()
    let n = 42
    println("count: " + n)
```

```miri,fails=MER_TYP_033,expects-message=Type 'int' does not have members
fn main()
    let n = 42
    println(n.to_string())
```

**Collections — length, push, first(), is_empty, contains:**

```miri
use system.collections.list

fn main()
    var l = List<int>()
    l.push(10)
    l.push(20)
    println(f"len {l.length()}")
    println(f"empty {l.is_empty()}")
    println(f"has {l.contains(10)}")
    match l.first()
        Some(v): println(f"first {v}")
        None: println("none")
```

**Map — index-set, get, iteration:**

```miri
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["a"] = 1
    m["b"] = 2
    println(f"size {m.length()}")
    match m.get("a")
        Some(v): println(f"a {v}")
        None: println("missing")
    for k in m
        println(f"key {k}")
```

Map iteration happens over keys. A common mistake is calling `.keys()`:

```miri,fails=MER_TYP_033,expects-message=has no field or method 'keys'
use system.collections.map

fn main()
    var m = Map<String, int>()
    for k in m.keys()
        println(k)
```

**String methods:**

```miri
fn main()
    let s = "  Hello, World  "
    let t = s.trim()
    println(t.to_lower())
    println(f"{t.length()}")
    let has = t.contains("World")
    println(f"{has}")
    for part in t.split(", ")
        println(part)
    match "42".to_int()
        Some(n): println(f"parsed {n}")
        None: println("not a number")
```

**Exiting non-zero.** Two ways, and they do not mix: declare `main() int` and
return the status, or call `panic(message)` from a void function to stop with
that message on stderr and status 1. `panic` is `void`, so it cannot be the tail
expression of a `main() int`. There is no `exit`:

```miri,fails=MER_TYP_034,expects-message=Undefined variable: exit
fn main()
    exit(3)
```

```miri
fn main() int
    let ok = false
    if ok
        return 0
    return 3
```

```miri
fn main()
    let ok = false
    if ok
        println("done")
    else
        panic("nothing to do")
```

## Verification Loop

**Loop:** `check --format json` → `fix --apply --yes` → re-check → `run` or `test`. Iterate until clean.

1. **miri check** — `miri check myfile.mi --format json`. The envelope gives each diagnostic a `code`, a `help` and, where one exists, a `repair`. A diagnostic carrying no `repair` is one you have to edit yourself, and that is the whole reason to read the envelope rather than the text.
2. **miri fix** — `miri fix myfile.mi --apply --yes` writes every repair the check recorded, in one call. `miri explain CODE` describes the rule behind one. `miri fix myfile.mi --plan --format json` previews the edits without writing them; it is a preview, not a step — the check already said which diagnostics carry a repair, so `--plan` adds a round trip and no information.
3. **miri run** / **miri test** — `miri run myfile.mi`, `miri test --dir <DIR>`; both take `--format json`. Only a run finds a fault the frontend cannot see.
4. **miri view** — read part of a file instead of all of it. `miri view myfile.mi --outline --public` gives the surface a caller can reach and is much smaller than the outline alone; `miri view myfile.mi --fn name` reads one function and `miri view myfile.mi --fn name --around text` narrows to the innermost block holding that text; `miri view --type Name --public` lists what can be called on a type; `miri view myfile.mi --raw --format json` returns the file's own bytes, comments and all, behind their line numbers. Without `--raw` the output is canonical, not literal, and a module name works wherever a path does.
5. **miri patch** — scoped edits: `miri patch myfile.mi --replace-in-fn name --old text --new text`. The edited program is re-checked before anything reaches disk.
6. **miri agent** — tool integration over JSON-RPC; `miri agent --help` carries the framing, the handshake and the method list. It does not run, build or test a program.

The auto-applicable repairs are:
- `add-import`: Import a name that resolves in exactly one module.
- `arrow-return-type`: Drop the `->` before a return type.
- `colon-annotation`: Drop the `:` before a type annotation.
- `concat-to-formatted-string`: Rewrite a `+` chain joining text to values as one f-string.
- `drop-extra-arguments`: Drop positional arguments a call does not declare.
- `drop-iterator-accessor`: Drop a `keys` accessor on a keyed collection and iterate it directly.
- `let-mut-to-var`: Rewrite a `let mut` binding as `var`.
- `let-to-var`: Rebind an immutable declaration as mutable.
- `null-to-none`: Rewrite `null`, `nil` or `nullptr` as `None`.
- `println-bang`: Drop the `!` from a macro-style call.
- `qualify-variant-pattern`: Prefix a bare variant pattern with the enum that declares it.
