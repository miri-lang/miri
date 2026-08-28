# Abstract Syntax Tree (AST)

The `ast` module defines the structural representation of parsed Miri code. It is the core data structure produced by the Parser and consumed by the Type Checker and MIR lowering phases.

## Overview

Unlike the linear stream of tokens produced by the Lexer, the AST forms a tree that explicitly represents the grammatical structure of the program. It captures the relationships between expressions, statements, and declarations.

## Architecture

-   **Expressions (`Expression`)**: Nodes representing computations that produce values, such as binary operations (`a + b`), function calls (`foo()`), literals (`42`), and memory access (`x[0]`).
-   **Statements (`Statement`)**: Nodes representing actions that do not explicitly yield a value in Miri, such as variable declarations (`let x = 10`), assignments (`x = 20`), and loops (`while x < 10: ...`).
-   **Declarations (`Declaration`)**: Top-level constructs that introduce new entities into the program, such as functions (`fn`), classes (`class`), enums (`enum`), and structs (`struct`).
-   **Types (`TypeExpression`)**: Syntactic representations of types as written by the programmer (e.g., `int`, `[String]`, `Map<K, V>`).

## Rendering back to source

`formatter/` renders an AST back to canonical Miri source, and `doc_comments.rs`
recovers the comments the lexer discards so an outline can show them.

The rendering is canonical rather than faithful: it is derived from the tree, so
comments, blank lines and the author's spacing are normalized away and one
program shape always produces one text. Its contract is that rendering is a
fixed point — text rendered from a tree parses back to a tree that renders to
the same text — which is what lets a tool read a declaration and later anchor an
edit against the bytes it read. `tests/ast/formatter.rs` holds that invariant
against every `.mi` file in the repository.

Rendering records spans as it goes, so each declaration comes with the byte range
it occupies **in the rendered text**, not in the file the tree was parsed from.

## Design Principles

1.  **Immutability**: AST nodes are mutated only by the single post-parse `normalize` pass (which rewrites collection `TypeKind` variants into `TypeKind::Custom`); thereafter they are treated as immutable. Passes like the Type Checker attach metadata (like inferred types) to a separate context rather than mutating the AST directly.
2.  **Span Tracking**: Every AST node contains an associated `Span` from the Lexer. This allows any downstream phase (Type Checker, MIR) to map errors directly back to the original source code location.
3.  **Strict Tree Structure**: The AST enforces the grammatical rules of Miri. Invalid syntactic constructs (e.g., placing a declaration inside an expression) are impossible to represent in the AST.
