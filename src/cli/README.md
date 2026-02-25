# CLI (Command Line Interface)

The `cli` module is the **entry point to the Miri compiler**.

## Overview

The CLI provides the interface for evaluating, compiling, and testing Miri programs. It serves as the orchestrator, initializing the compilation pipeline (Lexing -> Parsing -> Type Checking -> MIR Lowering -> Codegen) based on user commands.

## Features

-   **`miri run <file.mi>`**: Compiles and executes a specified Miri program.
-   **`miri build <file.mi>`**: Compiles a Miri program to a native executable binary.
-   **`miri check <file.mi>`**: Runs the frontend (Lexer, Parser, Type Checker) to validate code correctness without generating an executable.
-   **`miri repl`**: Starts an interactive Read-Eval-Print Loop for testing Miri expressions dynamically.

## Architecture

-   **Argument Parsing**: Utilizes the `clap` crate to define and parse subcommands, arguments, and flags (e.g., debug modes, optimization levels).
-   **Pipeline Invocation**: Bridges the CLI flags to the internal `CompilationPipeline`, steering backend selection and runtime loading.
-   **REPL Implementation**: Provides a lightweight shell loop for interpreting commands line-by-line using the `rustyline` crate.

## Design Principles

1.  **Developer Experience**: Commands should be intuitive and analogous to standard tools (like `cargo` or `rustc`).
2.  **Quick Feedback**: The `check` command provides immediate syntax and type validation.
3.  **Composability**: Internal components are decoupled so the CLI handles only presentation and flag parsing, not compilation logic.
