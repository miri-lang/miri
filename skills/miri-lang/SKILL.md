---
name: miri-lang
description: Writing Miri source files — syntax, type system, and iteration workflow
---

# Miri Language Essentials

This skill teaches the core syntax and idioms for writing `.mi` source files. It covers what the language supports, what it explicitly does not support, and the workflow for arriving at correct code.

## Positive Grammar

Miri's syntax is indentation-sensitive. Variables use space-separated type annotations (not colon form). Here is the core:

**Variables and functions:**

```miri
fn main()
    let x int = 5       // space-separated type annotation
    var y int = 10      // mutable with type
    y = 20
    
fn add(a int, b int) int
    a + b
```

**Structs:**

```miri
struct Point
    x int
    y int

fn main()
    let p = Point(3, 4)
```

**Enums:**

```miri
enum Color
    Red
    Green
    Blue

fn main()
    let c = Color.Red
```

**Match (exhaustive, no default):**

```miri
enum Status
    Active
    Inactive

fn process(s Status)
    match s
        Status.Active: println("running")
        Status.Inactive: println("stopped")
```

**Classes and methods:**

```miri
class Box
    fn open()
        println("opened")

fn main()
    let b = Box()
    b.open()
```

**Traits:**

```miri
trait Drawable
    fn draw()

class Circle implements Drawable
    fn draw()
        println("drawing circle")
```

**Generics:**

```miri
fn identity<T>(x T) T
    x

fn main()
    let n = identity(42)
    let s = identity("hello")
```

**Single-line blocks with colon:**

```miri
fn double(x int) int:
    x * 2
```

**Result (no import needed):**

```miri
fn main()
    let r = Result.Ok(5)
    match r
        Result.Ok(x): println("ok")
        Result.Err: println("error")
```

**Nullability with `T?` (no import needed):**

```miri
fn main()
    let x int? = None
    match x
        Some(v): v
        None: 0
```

**Collections (require imports):**

```miri
use system.collections.list

fn main()
    let empty = List<int>()
    let filled = List([1, 2, 3])
```

This core powers all Miri programs. The rest of this skill covers what the language **does not** support, because those gaps matter just as much as the syntax that works.


## Anti-Hallucination: Syntax That Does Not Exist

Miri deliberately leaves these off the surface to avoid ambiguity. Every example below is a compile error.

### Type Annotations on Bindings

The colon form does not exist. Use space-separated type annotations instead:

```miri,fails=MER_PAR_001
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

```miri,fails=MER_TYP_034
let x = 42i32
```

Miri infers numeric width from context. Annotate with an explicit cast when needed:

```miri
let x = 42
let y = 42 as i64
```

### Result and Option Match Arms: The Asymmetry

**Result** requires the constructor prefix (`Result.Ok`, `Result.Err`). Omitting it causes a type error:

```miri,fails=MER_TYP_038
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

```miri,fails=MER_TYP_036
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

```miri,fails=MER_TYP_043
let mut x = 5
```

Use `var`:

```miri
var x = 5
x = 10
```

### Parameters Are Immutable

Function parameters are immutable, even if the argument is mutable. To mutate the caller's variable, the method must exist in the stdlib or be written as a free function accepting a mutable reference pattern (not yet available). The closest common case:

```miri,fails=MER_TYP_042
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

## Verification Loop

After writing or editing any `.mi` file, always run:

```bash
miri check <file> --format json
```

Iterate on diagnostics until clean. The `--format json` flag gives machine-readable output; parse `diagnostics` for codes and fix messages:

```bash
# One-shot check
miri check myfile.mi --format json | jq '.diagnostics | length'

# If diagnostics exist, explain a code
miri explain MER_TYP_044

# Request a fix plan
miri fix --plan myfile.mi --format json
```

Running `miri fix --plan` on a file with errors prints structured repair suggestions. Many are auto-applicable; apply them with:

```bash
miri fix --apply --yes myfile.mi
```

The loop is: check → (if errors) explain + fix --plan → fix --apply → re-check. Stay in this loop until clean.
