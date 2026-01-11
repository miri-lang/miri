# Miri Project Overview

This document provides a high-level map of the Miri compiler codebase. It is intended to help contributors and AI assistants understand the project structure, architecture, and key design decisions.

## Architecture

Miri follows a standard compiler pipeline architecture:

`Source -> Lexer -> Parser -> AST -> Type Checking -> MIR Lowering -> MIR -> Codegen -> Object File -> Linker -> Executable`

### The Pipeline (`src/pipeline.rs`)

The centralized driver for the compiler is the `Pipeline` struct in `src/pipeline.rs`. It orchestrates:
1.  **Frontend**: Lexing and Parsing.
2.  **Script Wrapping**: Automatically wrapping top-level statements into a `main` function if no functions are defined.
3.  **Analysis**: Type checking.
4.  **Lowering**: Converting AST to MIR.
5.  **Backend**: Invoking Cranelift or LLVM to generate object code.
6.  **Linking**: Invoking the system linker (`cc`) to produce the final binary.

There is also an **Interpreter** path (`Pipeline::interpret` in `src/interpreter/`) which executes MIR directly without compiling to a binary.

## Key Modules

### 1. Frontend
-   **Lexer (`src/lexer/`)**: Uses `logos` to tokenize the input string.
-   **Parser (`src/parser/`)**: A recursive descent parser that produces the Abstract Syntax Tree (AST).
-   **AST (`src/ast/`)**: Defines the high-level syntax structure (Statements, Expressions, Declarations).

### 2. Analysis & IR
-   **Type Checker (`src/type_checker/`)**: Performs semantic analysis and type inference on the AST. Stores the type information in a `HashMap<usize, Type>`, which is separate from the AST. Each node in the AST has a unique index, which is used to access the type information in the map.
-   **MIR (`src/mir/`)**: **Mid-level Intermediate Representation**.
    -   This is a Control Flow Graph (CFG) based IR, similar to Rust's MIR.
    -   It uses **Basic Blocks** and **Terminators** (goto, return, branch) rather than nested AST structures.
    -   It is designed to support GPU execution models (see `GpuBodyMetadata`).
    -   **Lowering (`src/mir/lowering/`)**: Converts AST to MIR.

### 3. Backend (`src/codegen/`)
The backend is abstracted via the `Backend` trait.
-   **Cranelift (`src/codegen/cranelift/`)**: The *default* backend. Fast compilation, great for development. Generates object files directly in memory.
-   **LLVM (`src/codegen/llvm/`)**: Optional backend using `inkwell`. Intended for optimized production builds. Currently not implemented.

### 4. Runtime
-   **Intepreter (`src/interpreter/`)**: A stack-based interpreter that executes MIR directly. Useful for compile-time execution (const eval) or quick testing. Not meant for production use, but is aimed to have feature parity with the CPU/GPU backend.

## Key Libraries

*   **[Cranelift](https://cranelift.dev)**: The primary code generator. We use `cranelift-codegen`, `cranelift-frontend`, `cranelift-module`, etc.
*   **[Logos](https://github.com/maciejhirsz/logos)**: Used for generating the lexer.
*   **[Clap](https://github.com/clap-rs/clap)**: Handles command-line argument parsing in `src/cli/`.
*   **[Inkwell](https://github.com/TheDan64/inkwell)**: Safe wrapper around LLVM (optional).
*   **[Anyhow](https://github.com/dtolnay/anyhow) / [Thiserror](https://github.com/dtolnay/thiserror)**: Error handling.

## Key Design Decisions

*   **Script Mode**: Miri supports top-level code ("scripts"). The compiler detects this and wraps the code in a specialized `main` function before compilation.
*   **GPU First**: The MIR is designed with GPU concepts (Kernels, Memory Scope) in mind, preparing the language for heterogeneous computing.
*   **External Linker**: The project currently relies on the system's C compiler (`cc`) to link the object files produced by the backend. It does not use an embedded linker.
*   **Testing**: The project relies heavily on unit tests for each module, and the integration tests (in `tests/integration`) that run the full pipeline (both the interpreter and the backend) using `cargo-test`. Most testing modules have `utils.rs` file with common test utilities as well as the shared `tests/utils.rs` file.

## Directory Map

*   `src/ast`: Syntax tree definitions.
*   `src/cli`: Command line arguments handling.
*   `src/codegen`: Backend implementations (Cranelift/LLVM).
*   `src/error`: Various error types and formatting.
*   `src/interpreter`: Direct execution engine.
*   `src/lexer`: Source code tokenization.
*   `src/mir`: Intermediate representation definitions and lowering logic.
*   `src/parser`: Parsing logic.
*   `src/type_checker`: Type inference and validation.
*   `src/pipeline.rs`: The Pipeline that integrates all components.
*   `tests/cli`: CLI tests.
*   `tests/error`: Error formatting tests.
*   `tests/examples`: Miri programs to test the compiler.
*   `tests/integration`: Integration tests.
*   `tests/lexer`: Lexer tests.
*   `tests/mir`: MIR tests.
*   `tests/parser`: Parser tests.
*   `tests/type_checker`: Type checker tests.
*   `tests/interpreter`: Interpreter tests.
