# Abstract Syntax Tree (AST)

The `ast` module defines the structural representation of parsed Miri code. It is the core data structure produced by the Parser and consumed by the Type Checker and MIR lowering phases.

## Overview

Unlike the linear stream of tokens produced by the Lexer, the AST forms a tree that explicitly represents the grammatical structure of the program. It captures the relationships between expressions, statements, and declarations.

## Architecture

-   **Expressions (`Expression`)**: Nodes representing computations that produce values, such as binary operations (`a + b`), function calls (`foo()`), literals (`42`), and memory access (`x[0]`).
-   **Statements (`Statement`)**: Nodes representing actions that do not explicitly yield a value in Miri, such as variable declarations (`let x = 10`), assignments (`x = 20`), and loops (`while x < 10: ...`).
-   **Declarations (`Declaration`)**: Top-level constructs that introduce new entities into the program, such as functions (`fn`), classes (`class`), enums (`enum`), and structs (`struct`).
-   **Types (`TypeExpression`)**: Syntactic representations of types as written by the programmer (e.g., `int`, `[String]`, `Map<K, V>`).

## Design Principles

1.  **Immutability**: Once constructed by the parser, AST nodes are generally immutable. Passes like the Type Checker attach metadata (like inferred types) to a separate context rather than mutating the AST directly.
2.  **Span Tracking**: Every AST node contains an associated `Span` from the Lexer. This allows any downstream phase (Type Checker, MIR) to map errors directly back to the original source code location.
3.  **Strict Tree Structure**: The AST enforces the grammatical rules of Miri. Invalid syntactic constructs (e.g., placing a declaration inside an expression) are impossible to represent in the AST.
