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

```miri,fails=MER_PAR_001
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

## Module Resolution

Modules are located by dot-notation paths (e.g., `system.io` or `utils.math`). The search order determines where the compiler looks for each import:

### Bare Imports (`use util`, `use system.io`)

Searched in order:
1. **Standard library** (if available): `MIRI_STDLIB_PATH` env var, exe directory, or manifest directory (`src/stdlib` at build time)
2. **Project root**: The directory containing the entry file (the `.mi` file you pass to `miri run` or `miri check`)
3. **Current working directory**: Where the compiler was invoked from

Example: if you run `miri run src/main.mi` from the repo root, the project root is `src/`. A bare `use util` will first search the stdlib, then look for `src/util.mi`, then `./util.mi`.

**Standard library cannot be shadowed**: if both `src/system/io.mi` (a user file) and the real `system/io.mi` stdlib exist, the stdlib wins. Since `system` is a reserved keyword, local modules cannot use names from the `system` namespace.

### Local Imports (`use local.utils`, `use local.models.user`)

Resolved **only** against the project root (the entry file's directory). The working directory has no effect, so:
- `use local.utils` always resolves to the same file, no matter where the compiler is invoked from.
- Deeply nested imports like `use local.utils.math.calculations` work the same way: `calculations.mi` at `utils/math/calculations.mi` relative to the project root.

### Finding the Stdlib When CWD Matters

If you invoke `miri run /tmp/myproject/main.mi` from the `/` directory (not the repo root), the compiler must find the stdlib. It looks in this order:

1. `MIRI_STDLIB_PATH` environment variable (if you set it to the repo's `src/stdlib`)
2. A `stdlib/` directory next to the `miri` binary
3. Install prefix paths (`<prefix>/lib/miri/stdlib`)
4. **Manifest directory**: `src/stdlib` relative to the repo at build time (automatic fallback)

The fourth rule makes `miri run` work from any directory without `MIRI_STDLIB_PATH`, as long as the `miri` binary was built from the repo. If the stdlib is not found, the error will list all the roots that were searched and mention `MIRI_STDLIB_PATH` as an override.

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

Running `miri fix --plan` on a file with errors prints structured repair suggestions. The auto-applicable repairs are:
- `add-import`: Import a name that resolves in exactly one module.
- `arrow-return-type`: Drop the `->` before a return type.
- `colon-annotation`: Drop the `:` before a type annotation.
- `drop-extra-arguments`: Drop positional arguments a call does not declare.
- `let-mut-to-var`: Rewrite a `let mut` binding as `var`.
- `let-to-var`: Rebind an immutable declaration as mutable.
- `null-to-none`: Rewrite `null`, `nil` or `nullptr` as `None`.
- `println-bang`: Drop the `!` from a macro-style call.

Apply them with:

```bash
miri fix --apply --yes myfile.mi
```

The loop is: check → (if errors) explain + fix --plan → fix --apply → re-check. Stay in this loop until clean.
