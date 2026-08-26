# CLI (Command Line Interface)

The `cli` module is the **entry point to the Miri compiler**.

## Overview

The CLI provides the interface for evaluating, compiling, and testing Miri programs. It serves as the orchestrator, initializing the compilation pipeline (Lexing -> Parsing -> Type Checking -> MIR Lowering -> Codegen) based on user commands.

## Features

-   **`miri run <file.mi>`**: Compiles and executes a specified Miri program.
-   **`miri build <file.mi>`**: Compiles a Miri program to a native executable binary.
-   **`miri check <file.mi>`**: Runs the frontend (Lexer, Parser, Type Checker) to validate code correctness without generating an executable.
-   **`miri explain <CODE>`**: Renders the registry entry for a diagnostic code — the rule it enforces, a before/after pair, and a reference link.
-   **`miri fix <file.mi>`**: Reports the repairs the compiler recorded for a file (`--plan`, the default) or writes them (`--apply`). A repair classified as risky is refused unless `--allow-risky` is given.
-   **`miri agent`**: Serves JSON-RPC 2.0 over stdin and stdout so one process answers many requests. See [`docs/agent-protocol.md`](../../docs/agent-protocol.md).
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

## One command, three concerns

Each command module separates the work from the writing, because the command
line is no longer the only caller. A command exposes:

-   a **core** that runs the work and returns what it found — no printing, no
    process exit, no panic, so a long-lived session can call it;
-   a **rendering** step that turns that into text or an envelope;
-   a thin **`run`** that joins the two and is what `main.rs` calls.

`miri agent` calls the same cores, which is what keeps a session and the command
line reporting the same thing. `main.rs` marshals arguments and maps an outcome
onto an exit code; it holds no compilation logic.

## Architecture

-   **Argument Parsing**: Utilizes the `clap` crate to define and parse subcommands, arguments, and flags (e.g., debug modes, optimization levels).
-   **Pipeline Invocation**: Bridges the CLI flags to the internal `CompilationPipeline`, steering backend selection and runtime loading.

## Design Principles

1.  **Developer Experience**: Commands should be intuitive and similar to other programming languages' CLIs.
2.  **Quick Feedback**: The `check` command provides immediate syntax and type validation.
3.  **Composability**: Internal components are decoupled so the CLI handles only presentation and flag parsing, not compilation logic.
