# CLI (Command Line Interface)

The `cli` module is the **entry point to the Miri compiler**.

## Overview

The CLI provides the interface for evaluating, compiling, and testing Miri programs. It serves as the orchestrator, initializing the compilation pipeline (Lexing -> Parsing -> Type Checking -> MIR Lowering -> Codegen) based on user commands.

## Features

-   **`miri run <file.mi>`**: Compiles and executes a specified Miri program.
-   **`miri build <file.mi>`**: Compiles a Miri program to a native executable binary.
-   **`miri check <file.mi>`**: Runs the frontend (Lexer, Parser, Type Checker) to validate code correctness without generating an executable.
-   **`miri test`**: Discovers `@test` functions in `.mi` files under a directory and runs each one in an isolated subprocess, with optional filtering by `<path>::<test_name>` and a selectable output format (pretty/json). Discovery, execution and reporting live in `src/test_runner/`; see its README.

## Output Formats

All four commands (`check`, `build`, `run`, `test`) accept a `--format` flag to control output style:

-   **`--format pretty`** (default): Human-readable text output with optional ANSI color codes.
-   **`--format json`**: Machine-readable JSON envelope (`DiagnosticsEnvelope`) with all diagnostics and metadata. JSON output is always uncolored regardless of the `--color` flag.

## Color Output

Control ANSI color codes in terminal output with the global `--color` flag:

-   **`--color auto`** (default): Detect TTY and emit colors only if stderr is a terminal.
-   **`--color always`**: Force color codes on (useful when piping to tools that support ANSI).
-   **`--color never`**: Disable all color codes.

Note: JSON format (`--format json`) never emits ANSI escape codes regardless of the color setting.

## Architecture

-   **Argument Parsing**: Utilizes the `clap` crate to define and parse subcommands, arguments, and flags (e.g., debug modes, optimization levels).
-   **Pipeline Invocation**: Bridges the CLI flags to the internal `CompilationPipeline`, steering backend selection and runtime loading.

## Design Principles

1.  **Developer Experience**: Commands should be intuitive and similar to other programming languages' CLIs.
2.  **Quick Feedback**: The `check` command provides immediate syntax and type validation.
3.  **Composability**: Internal components are decoupled so the CLI handles only presentation and flag parsing, not compilation logic.
