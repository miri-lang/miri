# The Miri Programming Language

<p align="center">
  <img src="banner.png"/>
</p>

**A modern, GPU-first, statically-typed, compiled programming language designed for balancing high performance and safety in the age of Generative AI.**

Miri is designed for agentic engineering, where humans define intent and AI fills in safe, verifiable, high-performance implementations.

## Current State (v0.1.0-alpha.2)

Miri is in its second Alpha release. On top of the core language from Alpha 1, this release adds data types, collections, and memory management foundations.

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

*Note: Object-oriented features (classes, traits), closures, generics codegen, and GPU codegen are planned for upcoming milestones.*

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
Source → Lexer → Parser → AST → Type Checker → MIR → Codegen → Object File → Linker → Executable
```

The `Pipeline` struct in `src/pipeline.rs` orchestrates:

1. **Frontend** — Lexing and Parsing
2. **Script Wrapping** — Auto-wrapping top-level statements into `main` if needed
3. **Analysis** — Type checking
4. **Lowering** — Converting AST to MIR
5. **Backend** — Cranelift (default) code generation
6. **Linking** — System linker (`cc`) produces the final binary

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
