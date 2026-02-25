# Error

The `error` module contains the **unified error handling and diagnostic reporting system** for the Miri compiler.

## Overview

Miri emphasizes actionable, context-rich error messages. Instead of simply reporting a string, errors in Miri are typed, structured, and inherently tied back to the original source code location (Span).

## Architecture

-   **MiriError (`MiriError`)**: The central error enum that encompasses all possible compiler failures—from lexical analysis and syntax errors to type checking constraints and codegen failures.
-   **Spans (`Span`)**: Every error carries a `Span`, which represents the exact byte range in the source file where the error occurred.
-   **Diagnostic Formatting (`reporter.rs`/`format.rs`)**: Responsible for presenting the `MiriError` to the console in a human-readable format, often drawing the exact lines of code that failed and appending helpful hints.
-   **Accumulation**: The compiler attempts to collect multiple errors across a single pass (like parsing or type-checking) rather than halting immediately, providing the developer with comprehensive feedback.

## Design Principles

1.  **Actionable Feedback**: Errors specifically suggest fixes or point to why an operation failed (e.g., "Expected type `int`, found `string`").
2.  **No Panics**: The compiler components themselves are designed to return a `Result<T, MiriError>` instead of panicking (`unwrap`/`expect`), ensuring stability and graceful error reporting for invalid code.
3.  **Domain Separation**: Errors are cleanly categorized based on the phase they occurred in (Lexer, Parser, TypeChecker, Compiler/Backend).
