# Miri

**A modern, GPU-first, statically-typed programming language designed for balancing high performance and developer productivity in the age of GenAI.**

Miri combines the readability of Ruby and Python with the safety and speed of Rust—essential in the age of Generative AI, when code is often written by machines and must be understood by humans.

## Features

- **Indentation-Sensitive Syntax** — Clean, readable code without curly braces
- **Static Typing** — Catch errors at compile time with powerful type inference
- **Immutable by Default** — Variables are immutable unless declared with `var`
- **Null Safety** — No nulls; safety is built into the type system
- **GPU-First Design** — Miri's intermediate representation (MIR) is designed with heterogeneous computing in mind (kernels, memory scope)
- **Script Mode** — Run top-level code directly without boilerplate

## Quick Start

### Hello World

```miri
use system.io.{println}

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

### Collections

```miri
let array = [1, 2, 3]
let list List<int> = [1, 2, 3]
let map = {"key": "value"}
let tuple = (1, true)
let set = {1, 2, 3}
```

### Structs

```miri
struct User
    id int
    name string

let u = User(id: 1, name: "Alice")
```

## Modules and Imports

Miri uses a dot-separated path syntax for imports. The folder structure defines the namespace.

```miri
// Standard library — prefixed with `system`
use system.io.{print, println}
use system.math as M
use system.net.*

// Local project modules — prefixed with `local`
use local.users.user
use local.utils.{helpers, validators as V}

// External packages — package name as prefix
use my_package.module
use some_lib.feature.{Component, render}
```

### Import Features

- **Single import**: `use system.math`
- **Aliasing**: `use system.math as M`
- **Multi-import**: `use system.{io, net, text as T}`
- **Wildcard**: `use system.io.*`

## Architecture

Miri follows a standard compiler pipeline:

```
Source → Lexer → Parser → AST → Type Checker → MIR → Codegen → Object File → Linker → Executable
```

The `Pipeline` struct in `src/pipeline.rs` orchestrates:

1. **Frontend** — Lexing and Parsing
2. **Script Wrapping** — Auto-wrapping top-level statements into `main` if needed
3. **Analysis** — Type checking
4. **Lowering** — Converting AST to MIR
5. **Backend** — Cranelift (default) or LLVM code generation
6. **Linking** — System linker (`cc`) produces the final binary

An **Interpreter** path (`Pipeline::interpret`) executes MIR directly without compilation.

### Key Modules

| Module | Path | Description |
|--------|------|-------------|
| Lexer | `src/lexer/` | Tokenization using [Logos](https://github.com/maciejhirsz/logos) |
| Parser | `src/parser/` | Recursive descent parser producing AST |
| AST | `src/ast/` | High-level syntax tree definitions |
| Type Checker | `src/type_checker/` | Semantic analysis and type inference |
| MIR | `src/mir/` | CFG-based intermediate representation |
| Codegen | `src/codegen/` | Backend implementations |
| Interpreter | `src/interpreter/` | Stack-based MIR execution |

### Backends

- **Cranelift** (`src/codegen/cranelift/`) — Default backend. Fast compilation for development.
- **LLVM** (`src/codegen/llvm/`) — Optional backend via [Inkwell](https://github.com/TheDan64/inkwell). Intended for optimized production builds (not yet implemented).

## Repository Layout

```
src/
├── ast/          # Syntax tree definitions
├── cli/          # Command-line interface
├── codegen/      # Backend implementations (Cranelift/LLVM)
├── error/        # Error types and formatting
├── interpreter/  # Direct MIR execution engine
├── lexer/        # Source tokenization
├── mir/          # IR definitions and lowering
├── parser/       # Parsing logic
├── type_checker/ # Type inference and validation
└── pipeline.rs   # Main compiler driver

tests/
├── cli/          # CLI tests
├── error/        # Error formatting tests
├── examples/     # Example Miri programs
├── integration/  # Full pipeline tests
├── interpreter/  # Interpreter tests
├── lexer/        # Lexer tests
├── mir/          # MIR tests
├── parser/       # Parser tests
└── type_checker/ # Type checker tests
```

## Building from Source

Miri is written in Rust. Build with a stable Rust toolchain:

```bash
cargo build --release
```

The binary will be available at `target/release/miri`.

## Running Tests

Run the full test suite:

```bash
cargo test
```

## Contributing

We welcome contributions! Please read our [Contributing Guide](CONTRIBUTING.md) for details on code style, testing requirements, and the submission process.

## License

[Apache-2.0](LICENSE)
