# Error

The `error` module contains the **unified error handling and diagnostic reporting system** for the Miri compiler.

## Overview

Miri emphasizes actionable, context-rich error messages. Instead of simply reporting a string, errors in Miri are typed, structured, and inherently tied back to the original source code location (Span).

## Architecture

-   **CompilerError (`CompilerError`)**: The top-level error enum that encompasses all possible compiler pipeline failures—from I/O and lexical analysis to type checking constraints, codegen, and runtime failures.
-   **Spans (`Span`)**: Core error types like `SyntaxError` and `TypeError` carry a `Span` representing the exact byte range in the source file where the error occurred.
-   **Diagnostic System (`diagnostic.rs`)**: Provides foundational types (`Severity`, `Diagnostic`, `Reportable`) for rich, human-readable error messages with context, help text, and notes. Each diagnostic carries a stable code (e.g., `MER_TYP_010`) for documentation and tooling integration.
-   **Diagnostic Formatting (`format.rs`)**: Responsible for presenting diagnostics to the console, often drawing the exact lines of code that failed and appending helpful hints. Color output is controlled via the `--color` CLI flag (`auto`/`always`/`never`).
-   **JSON Serialization (`src/diagnostics/json.rs`)**: Converts diagnostics to a machine-readable `DiagnosticsEnvelope` with all errors, warnings, and metadata. The schema is defined in `docs/diagnostics-schema.json` (JSON Schema draft 2020-12).

## Design Principles

1.  **Actionable Feedback**: Diagnostics specifically suggest fixes or point to why an operation failed (e.g., "Expected type `int`, found `string`").
2.  **No Panics**: The compiler components themselves are designed to return a `Result<T, E>` (where `E` forms part of `CompilerError`) instead of panicking (`unwrap`/`expect`), ensuring stability and graceful error reporting for invalid code.
3.  **Domain Separation**: Errors are cleanly categorized based on the phase they occurred in (Syntax/Lexer/Parser, TypeChecker, Lowering, Codegen, Runtime).
