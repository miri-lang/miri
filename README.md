# The Miri Programming Language

<p align="center">
  <img src="banner.png"/>
</p>

**A modern, GPU-first, statically-typed, compiled programming language designed for balancing high performance and safety in the age of Generative AI.**

Miri is designed for agentic engineering, where humans define intent and AI fills in safe, verifiable, high-performance implementations.

## Current State (v0.2.0-alpha.4)

Miri is in its fourth Alpha release. On top of OOP, closures, and generics from Alpha 3, this release adds a full multi-file module system with cross-module visibility, module aliasing, and robust error diagnostics.

**Working Features:**
- **Primitives & Variables**: `int`, `float`, `bool`, `String` via `let` (immutable) and `var` (mutable).
- **Functions**: Typed parameters and returns, named arguments.
- **Control Flow**: `if/else`, `unless`, `while`, `until`, `do-while`, `forever`, `for..in`.
- **Pattern Matching**: `match` with guards, destructuring, and or-patterns.
- **Structs**: Named fields, construction with named arguments, field access.
- **Enums**: Variants with associated data, pattern matching with extraction.
- **Tuples**: Construction, index access, destructuring in match.
- **Collections**: `Array` (fixed-size `[T; N]`), `List` (dynamic `[T]`), `Map` (`{K: V}`), `Set` (`{T}`) — all with full method APIs.
- **Option Types**: `Type?`, `None`, `Some`, `if let` unwrapping.
- **Type Aliases**: `type ID is String`.
- **Memory Model**: Container-level reference counting, auto-copy for small types, drop specialization.
- **Compilation Pipeline**: Full frontend (Lexer, Parser, Type Checker), MIR Lowering with 5 optimization passes, and Native Codegen (via Cranelift).
- **Classes**: Full OOP with constructors (`init`), methods, field access, visibility modifiers (`private`, `protected`, `public`), inheritance with complete field layout, `super` method calls, and abstract class/method enforcement.
- **Traits**: Declare shared interfaces with abstract and concrete (default) methods. Classes implement one or more traits; trait inheritance chains are fully validated by the type checker.
- **Closures**: Non-capturing and capturing lambdas compiled to native code. Captures by value; closure represented as a fat pointer `(fn_ptr, env_ptr)`.
- **Generics**: Generic function and generic struct/class monomorphization. Specialized copies emitted per unique type instantiation.
- **Virtual Dispatch**: Vtable generation for class hierarchies; runtime method dispatch for polymorphic variables and trait objects.
- **Multi-File Projects**: Programs can span multiple `.mi` files. The compiler discovers, parses, and links all files in a project automatically.
- **Module System**: `use local.*` resolves to project files, `use system.*` resolves to stdlib. Supports selective imports (`use system.io.{println}`) and module aliasing (`use system.math as M`).
- **Cross-Module Visibility**: `public`, `private`, and `protected` modifiers are enforced across module boundaries. Private symbols are invisible to importers.
- **Namespace Collision Detection**: Conflicting names across imports and local declarations are detected with clear error messages and suggestions.
- **Circular Dependency Detection**: Circular import chains are detected and reported with clear diagnostics.

*Note: GPU codegen, closures with capture-by-reference, and full memory safety (Perceus+) are planned for upcoming milestones.*

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
