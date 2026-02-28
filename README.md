# The Miri Programming Language

**A modern, GPU-first, statically-typed, compiled programming language designed for balancing high performance and safety in the age of Generative AI.**

Miri is designed for agentic engineering, where humans define intent and AI fills in safe, verifiable, high-performance implementations.

## Current State (v0.1.0-alpha.1)

Miri is currently in its first Alpha release, supporting foundational language features.

**Working Features:**
- **Primitives & Variables**: `int`, `float`, `bool`, `String` via `let` (immutable) and `var` (mutable).
- **Functions**: Typed parameters and returns.
- **Control Flow**: `if/else`, `unless`, `while`, `until`, `do-while`, `forever`, `for..in`.
- **Pattern Matching**: The `match` statement.
- **Compilation Pipeline**: Full frontend (Lexer, Parser, Type Checker), MIR Lowering, and Native Codegen (via Cranelift).

*Note: Collections (lists, maps, tuples), object-oriented features (classes, traits, structs), and GPU codegen are planned for upcoming milestones.*

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
cargo build --release
```

The binary will be available at `target/release/miri`.

## Running Tests

Run the full test suite for the compiler and standard library:

```bash
cargo test
```

To run tests for the runtime components (like the Miri core runtime), navigate to the runtime directory:

```bash
cd src/runtime/core
cargo test
```

## Contributing

We welcome contributions! Please read our [Contributing Guide](CONTRIBUTING.md) for details on code style, testing requirements, and the submission process.

### Contributors

- Viacheslav Shynkarenko aka Slavik Shynkarenko (maintainer)

## License

[Apache-2.0](LICENSE)
