# Type Checker

The `type_checker` module enforces Miri's static typing rules, ensuring that expressions are valid and operations are performed on compatible types before code generation begins.

## Overview

The Type Checker traverses the Abstract Syntax Tree (AST), assigns types to every expression, and validates that all type constraints (e.g., function arguments, assignments, field accesses) are satisfied. It also performs type inference.

## Architecture

-   **Context Environment (`Context`)**: The central state object that tracks variables, functions, and types currently in scope. It handles lexical scoping by pushing and popping environments as the traversal enters and leaves blocks.
-   **Validation Passes**: The checker validates declarations top-level constructs, statements, and deeply inspects expressions.
-   **Type Inference**: For variable declarations without explicit types (`let x = 42`), the type checker infers the type from the right-hand-side expression.
-   **Generics (`generics.rs`)**: Handles substitution and validation of type parameters for generic functions and collections.
-   **Visibility Verification**: Enforces access control (`public`, `private`, `protected`) for class fields and methods across module boundaries.

## Design Principles

1.  **Multiple Errors**: Like the parser, the type checker is designed to report as many type errors as possible in a single run, utilizing a unified diagnostic system rather than failing immediately upon the first error.
2.  **Non-Destructive AST**: The type checker does not heavily mutate the AST. Type information necessary for later stages (MIR lowering) is either stored in a sidecar data structure (the Context) or annotated minimally.
3.  **Strictness**: It strictly enforces type compatibility, trait constraints, and OOP inheritance rules, guaranteeing that well-typed Miri programs will not encounter runtime type exceptions.
