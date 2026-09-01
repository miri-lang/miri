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

Miri deliberately leaves these off the surface to avoid ambiguity. Every example below is a compile error.

### Type Annotations on Bindings

The colon form does not exist. Use space-separated type annotations instead:

```miri,fails=MER_PAR_001,expects-message=Expected an expression, but found :
let x: int = 5
```

Use space-separated type annotations:

```miri
let x int = 5
let y int = 10
```

If the type cannot be inferred, annotate with a cast:

```miri
let z = 5 as i64  // explicit cast when needed
```

### Literal Type Suffixes

No suffix syntax like `1i32` or `3.14f64`:

```miri,fails=MER_TYP_034,expects-message=Undefined variable: i32
let x = 42i32
```

Miri infers numeric width from context. Annotate with an explicit cast when needed:

```miri
let x = 42
let y = 42 as i64
```

### Result and Option Match Arms: The Asymmetry

**Result** requires the constructor prefix (`Result.Ok`, `Result.Err`). Omitting it causes a type error:

```miri,fails=MER_TYP_038,expects-message=Expected enum variant pattern like EnumName.Ok
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

Bare imports are searched in this order: stdlib (via `MIRI_STDLIB_PATH` env var), the entry file's directory, then the current working directory. Local imports (`use local.*`) resolve only against the entry file's directory. The stdlib cannot be shadowed; `system` is reserved. Set `MIRI_STDLIB_PATH` to override the stdlib location when the binary is not beside `stdlib/`.

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

**Exit codes from main:**

```miri
fn main() int
    let ok = false
    if ok
        return 0
    return 3
```

## Verification Loop

The workflow for writing correct Miri code:

1. **miri check** — compile and collect diagnostics: `miri check myfile.mi --format json`
2. **miri run** — execute and see real output: `miri run myfile.mi`
3. **miri test** — run tests in a directory: `miri test --dir <DIR>`
4. **miri view** — read scoped code: `miri view myfile.mi --outline` or `--fn name`
5. **miri patch** — make scoped edits: `miri patch myfile.mi --replace-in-fn name --old text --new text`
6. **miri fix** — repair errors: `miri explain CODE`, `miri fix --plan myfile.mi`, `miri fix --apply --yes myfile.mi`
7. **miri agent** — tool integration (see `tools/agent_client.py` and `docs/agent-protocol.md`)

**Loop:** check → (if errors) explain + fix --plan → fix --apply → run/test → re-check. Iterate until clean.

The auto-applicable repairs are:
- `add-import`: Import a name that resolves in exactly one module.
- `arrow-return-type`: Drop the `->` before a return type.
- `colon-annotation`: Drop the `:` before a type annotation.
- `drop-extra-arguments`: Drop positional arguments a call does not declare.
- `let-mut-to-var`: Rewrite a `let mut` binding as `var`.
- `let-to-var`: Rebind an immutable declaration as mutable.
- `null-to-none`: Rewrite `null`, `nil` or `nullptr` as `None`.
- `println-bang`: Drop the `!` from a macro-style call.
