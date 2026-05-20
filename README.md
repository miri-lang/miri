# The Miri Programming Language

<p align="center">
  <img src="banner.png"/>
</p>

**A modern, GPU-first, statically-typed, compiled programming language designed for balancing high performance and safety in the age of Generative AI.**

Miri is built for **agentic engineering** — a world in which the majority of production code is generated, repaired, and shipped by autonomous agents. Humans declare intent. The compiler enforces the invariants agents are most likely to violate, and the toolchain emits structured artifacts agents can consume directly.

## Current State (v0.4.0-beta.2)

Miri's second Beta release lays down the **Core Standard Library** that every future stdlib module and the GPU preview will build on. Building on the memory-safety guarantees from Beta 1, this release ships `Result<T, E>` with compiler-enforced `must_use`, a backend-agnostic `system.math`, a focused four-trait taxonomy for collection transforms (`Queryable`, `Transformable`, `Foldable`, `Sequenced`), and an intrinsic-backed `system.testing` module so test assertions can live directly in `.mi` sources.

**New in v0.4.0-beta.2:**
- **`Result<T, E>`**: Enum with `Ok(T)` / `Err(E)` variants and `is_ok`, `is_err`, `unwrap_or` methods. The compiler enforces `must_use` semantics — ignoring a `Result` is a compile error. Auto-propagation forces every fallible API to be inspected.
- **`system.math`**: `abs`, `min`, `max`, `pow`, `sqrt`, `floor`, `ceil`, `round`, `sin`, `cos`, `tan`, `log`, `exp` plus `PI`, `E`, `INF` as free functions. One source, two lowerings — calls inside `gpu fn` (M6.5) will route to WGSL/SPIR-V built-ins; calls on the CPU lower to `libm` / Cranelift intrinsics.
- **Collection trait taxonomy**: Four focused traits replace the old kitchen-sink design.
  - `Queryable<T>` — `is_empty`, `first`, `last`, `contains`, `index_of`.
  - `Transformable<T>` — `map`, `filter`, `flat_map`.
  - `Foldable<T>` — `reduce`, `any`, `all`, `count_where`, `sum`, `min`, `max`. Empty-collection-safe: `sum`/`min`/`max` return `T?` (`None` on empty).
  - `Sequenced<T>` extends `Transformable<T>` — `take`, `skip`, `sorted_by`, `unique`, `reversed`, `zip`, `enumerate`.
  - `List<T>` and `Array<T, N>` inherit default trait bodies; `Map<K, V>` and `Set<T>` keep ad-hoc `map`/`filter`/`reduce`. `GpuArray<T>` (M6.5) will implement the same traits without API churn.
- **`system.testing`**: `assert(cond, msg?)`, `assert_eq<T>(actual, expected, msg?)`, `assert_ne<T>(a, b, msg?)`, and `assert_panics(f, expected?)` declared `intrinsic`. Failures abort with `Runtime error: assertion failed at <path>:<line>: <detail>`. `assert_panics` catches Miri-level `panic(...)` via a setjmp/longjmp frame.
- **Trait & generic improvements**: Soft-cycle module loading, two-phase pre-pass for forward references, trait-default re-lowering per concrete class, nested generic args in `implements` / `extends` clauses, and base-class generic substitution (`class IntStack extends Stack<int>`).

**Working Features:**
- **Primitives & Variables**: `int`, `float`, `bool`, `String` via `let` (immutable) and `var` (mutable).
- **Functions**: Typed parameters and returns, named arguments. `out` parameters for in-place mutation (`fn inc(n out int): n = n + 1`).
- **Control Flow**: `if/else`, `unless`, `while`, `until`, `do-while`, `forever`, `for..in`.
- **Pattern Matching**: `match` with guards, destructuring, and or-patterns.
- **Structs**: Named fields, construction with named arguments, field access.
- **Enums**: Variants with associated data, pattern matching with extraction.
- **Tuples**: Construction, index access, destructuring in match.
- **Collections**: `Array` (fixed-size `[T; N]`), `List` (dynamic `[T]`), `Map` (`{K: V}`), `Set` (`{T}`) — full method APIs, value semantics enforced by Copy-on-Write, and the four-trait functional pipeline (`map`/`filter`/`reduce`/`zip`/`enumerate`/`sum`/`min`/`max`/`take`/`skip`/`sorted_by`/`unique`/`reversed`).
- **`Result<T, E>`**: Compiler-enforced `must_use`, `Ok` / `Err` pattern matching, `is_ok` / `is_err` / `unwrap_or` helpers.
- **`system.math`**: Backend-agnostic numeric primitives + constants (`PI`, `E`, `INF`).
- **`system.testing`**: Intrinsic `assert`, `assert_eq`, `assert_ne`, `assert_panics`.
- **Option Types**: `Type?`, `None`, `Some`, `if let` unwrapping. `?? default` for fallback.
- **Type Aliases**: `type ID is String`.
- **Memory Safety (Perceus+)**: Compiler-inferred ownership with element-level reference counting, automatic IncRef/DecRef placement, RC elision on linear flow, and Copy-on-Write for collections and strings. No memory annotations required (only `out`).
- **Use-After-Move Checking**: Resource types (any type defining `fn drop(self)`) are tracked strictly at every scope. Managed types (collections, strings, classes) are tracked at top level immediately and inside function bodies via escape inference — multi-hop diagnostic chains explain *why* a value is consumed (which call → which sink).
- **Cloneable Trait & `.clone()`**: Built-in `Cloneable` trait with deep-copy semantics. All managed types implement it; user-defined classes get auto-generated `__clone_TypeName` helpers.
- **User-Defined Destructors**: `fn drop(self)` on a struct or class declares a resource type. Drop fires automatically at scope exit before recursive field decref and free (RC=0 → user `drop` → field DecRef → free).
- **Compilation Pipeline**: Full frontend (Lexer, Parser, Type Checker), MIR Lowering with 5 optimization passes, and Native Codegen (via Cranelift).
- **Classes**: Full OOP with constructors (`init`), methods, field access, visibility modifiers (`private`, `protected`, `public`), inheritance with complete field layout (including base-class generic substitution), `super` method calls, and abstract class/method enforcement.
- **Traits**: Declare shared interfaces with abstract and concrete (default) methods. Classes implement one or more traits; trait inheritance chains are fully validated by the type checker. Nested generic args in `implements` / `extends` clauses (`class Box<T> implements Iterable<List<T>>`) resolve correctly.
- **Closures**: Non-capturing and capturing lambdas compiled to native code. Captures by value; closure represented as a fat pointer `(fn_ptr, env_ptr)`. Captured values are RC-tracked and released when the closure is dropped.
- **Generics**: Generic function and generic struct/class monomorphization. Specialized copies emitted per unique type instantiation. Per-instantiation vtables for `class<T> implements Trait`.
- **Virtual Dispatch**: Vtable generation for class hierarchies; runtime method dispatch for polymorphic variables and trait objects. `out` parameters work across virtual / trait method calls.
- **Multi-File Projects**: Programs can span multiple `.mi` files. The compiler discovers, parses, and links all files in a project automatically.
- **Module System**: `use local.*` resolves to project files, `use system.*` resolves to stdlib. Supports selective imports (`use system.io.{println}`), module aliasing (`use system.math as M`), and soft-cycle loading for mutually-dependent stdlib modules.
- **Cross-Module Visibility**: `public`, `private`, and `protected` modifiers are enforced across module boundaries. Private symbols are invisible to importers.
- **Namespace Collision Detection**: Conflicting names across imports and local declarations are detected with clear error messages and suggestions.
- **Circular Dependency Detection**: Circular import chains are detected and reported with clear diagnostics.

## Quick Start

### Hello World

```miri
use system.io

fn main()
    println("Hello, World!")
```

### Variables

```miri
let x = 10              // Immutable integer
var y = 20              // Mutable integer
y = 30                  // OK
let name String = "Miri" // Explicit type
```

### Functions

```miri
fn add(a int, b int) int
    a + b

let result = add(5, 10)
```

### Structs

```miri
use system.io

struct Point
    x int
    y int

fn offset(p Point, dx int, dy int) Point
    Point(x: p.x + dx, y: p.y + dy)

fn main()
    let p = Point(x: 1, y: 2)
    let q = offset(p, 10, 20)
    println(f"{q.x}, {q.y}")
```

### Enums with Data

```miri
use system.io

enum Shape
    Circle(float)
    Rect(float, float)

fn area(s Shape) float
    match s
        Shape.Circle(r): 3.14 * r * r
        Shape.Rect(w, h): w * h
```

### Collections

```miri
use system.io
use system.collections.list
use system.collections.map

var items = List([1, 2, 3])
items.push(4)

let scores = {"Alice": 95, "Bob": 87}
println(f"{scores["Alice"]}")
```

### Classes

```miri
use system.io

class Animal
    protected name String

    fn init(n String)
        self.name = n

    fn speak()
        println(f"I am {self.name}")

class Dog extends Animal
    fn speak()
        super.speak()
        println("Woof!")

fn main()
    let d = Dog(n: "Rex")
    d.speak()
```

### Closures

```miri
use system.io

fn main()
    var x = 10
    let add = fn(n int) int: x + n
    println(f"{add(5)}")   // 15
```

### Generics

```miri
use system.io

fn identity<T>(x T) T
    x

struct Wrapper<T>
    value T

fn main()
    println(f"{identity(42)}")
    println(f"{identity("hello")}")
    let w = Wrapper<int>(value: 99)
    println(f"{w.value}")
```

### Memory Safety in Practice

Value semantics are enforced — assignment is a logical copy, mutation never aliases:

```miri
use system.io
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a            // copy-on-write share
    b.push(4)            // CoW fires: b becomes independent
    println(f"{a.length()} {b.length()}")   // 3 4
```

`out` is the one explicit memory annotation:

```miri
use system.io

fn inc(n out int)
    n = n + 1

fn main()
    var x = 41
    inc(x)
    println(f"{x}")      // 42
```

Resource types (those defining `fn drop(self)`) are tracked strictly — the compiler refuses to let you use one after it has been consumed:

```miri
struct File
    handle int
    fn drop(self)
        // close the underlying handle
        ...

fn archive(f File)
    // ...

fn main()
    let f = File(handle: 1)
    archive(f)
    archive(f)           // compile error: 'f' was consumed by 'archive'
```

### Traits

```miri
use system.io

trait Speakable
    fn speak()

trait Describable extends Speakable
    fn describe()
        println("I am an animal")

class Dog implements Describable
    fn speak()
        println("Woof!")

fn main()
    let d = Dog()
    d.speak()
    d.describe()
```

### Multi-File Projects

```miri
// models/user.mi
use system.io

class User
    public name String

    fn init(n String)
        self.name = n

    public fn greet()
        println(f"Hello, {self.name}")
```

```miri
// main.mi
use local.models.user

fn main()
    let u = User(n: "Alice")
    u.greet()
```

### Module Aliasing

```miri
use system.collections.list as L

fn main()
    var items = L.List([1, 2, 3])
```

### Option Types

```miri
use system.io

fn find(name String?)
    if let Some(s) = name
        println(f"Found: {s}")
```

### Control Flow

```miri
if x > 10
    print("Big")
else
    print("Small")

for i in 1..5
    print(i)
```

### Pattern Matching

```miri
match x
    1: print("One")
    2 | 3: print("Two or Three")
    x if x > 10: print("Large")
    _: print("Other")
```

### `Result<T, E>`

```miri
use system.io
use system.result

fn divide(a int, b int) Result<int, String>
    if b == 0
        return Result.Err("division by zero")
    return Result.Ok(a / b)

fn main()
    match divide(10, 0)
        Result.Ok(v): println(f"got {v}")
        Result.Err(e): println(f"err: {e}")
```

Ignoring a `Result` value without inspecting it is a compile error — fallible APIs cannot be silently dropped.

### `system.math`

```miri
use system.io
use system.math as M
use system.math.{sqrt, pow}

fn main()
    println(f"{sqrt(2.0)}")
    println(f"{pow(2.0, 10.0)}")
    println(f"{M.PI}")
```

### Functional Collection Pipelines

```miri
use system.io
use system.collections.list

fn main()
    let xs = List([1, 2, 3, 4, 5])
    let squared_evens = xs.filter(fn(x int) bool: x % 2 == 0).map(fn(x int) int: x * x)
    println(f"{squared_evens.sum() ?? 0}")   // 20

    let pairs = List([1, 2, 3]).zip(List([10, 20, 30]))
    let indexed = List(["a", "b", "c"]).enumerate()
```

`sum` / `min` / `max` return `T?` — `None` on an empty collection. Combine with `?? default` for a one-shot fallback.

### Testing

```miri
use system.io
use system.testing

fn add(a int, b int) int
    a + b

fn main()
    assert_eq(add(2, 3), 5)
    assert(add(1, 1) == 2, "addition should be commutative")
    assert_panics(fn(): panic("boom"), "boom")
```

A failing `assert_eq` aborts with `Runtime error: assertion failed at <path>:<line>: expected 5, got 6`.

## Architecture

Miri follows a standard compiler pipeline:

```text
Source(s) → Lexer → Parser → AST → Type Checker → MIR → Codegen → Object File → Linker → Executable
```

The `Pipeline` struct in `src/pipeline.rs` orchestrates:

1. **Discovery** — Finding all `.mi` files in the project, resolving `use local.*` and `use system.*` imports
2. **Frontend** — Lexing and Parsing (per file)
3. **Script Wrapping** — Auto-wrapping top-level statements into `main` if needed
4. **Analysis** — Type checking with cross-module visibility enforcement
5. **Lowering** — Converting AST to MIR
6. **Backend** — Cranelift (default) code generation
7. **Linking** — System linker (`cc`) produces the final binary

### Backends

- **Cranelift** (`src/codegen/cranelift/`) — Default backend. Fast compilation for development.
- *(Future)* **LLVM** (`src/codegen/llvm/`) — Intended for optimized production builds.

## Repository Layout

```bash
src/
├── ast/          # Syntax tree definitions
├── cli/          # Command-line interface
├── codegen/      # Backend implementations (Cranelift)
├── error/        # Error types and formatting
├── lexer/        # Source tokenization
├── mir/          # IR definitions and lowering
├── parser/       # Parsing logic
├── runtime/      # Scaffolding and intrinsics
├── stdlib/       # Standard Library (system.*)
├── type_checker/ # Type inference and validation
└── pipeline.rs   # Main compiler driver
```

## Building from Source

Miri is written in Rust. Build with a stable Rust toolchain:

```bash
make build        # Build compiler + all runtime crates (debug)
make release      # Build compiler + all runtime crates (release)
```

The release binary will be available at `target/release/miri`.

## Running Tests

Run the full test suite across the compiler, standard library, and all runtime crates:

```bash
make test
```

## Linting & Formatting

```bash
make lint         # Check formatting + clippy (compiler + runtimes)
make format       # Auto-format all code (compiler + runtimes)
```

## Contributing

We welcome contributions! Please read our [Contributing Guide](CONTRIBUTING.md) for details on code style, testing requirements, and the submission process.

### Contributors

- Viacheslav Shynkarenko aka Slavik Shynkarenko (maintainer)

## License

[Apache-2.0](LICENSE)
