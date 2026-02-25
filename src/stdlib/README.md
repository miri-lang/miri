# Standard Library (Stdlib)

The `stdlib` module contains the **Miri Standard Library**, written in Miri itself.

## Overview

The Standard Library provides the built-in functionality available to all Miri programs. It sits on top of the generic `runtime` module, offering a rich, type-safe API for common programming tasks.

## Architecture

-   **`system.io`**: Utilities for reading and writing to standard input, standard output, and standard error (e.g., `print`, `println`).
-   **`system.string`**: The core `String` class, extending the runtime representation with methods for concatenation, comparison, and formatting.
-   **Collections (Planned)**: Modules for `List`, `Map`, `Set`, `Tuple` etc., implemented using runtime intrinsics.
-   **Math (Planned)**: Standard mathematical constants and functions (`sin`, `cos`, `abs`, etc.).

## Design Principles

1.  **Miri Native**: The standard library is overwhelmingly written in Miri, leveraging the compiler's own type system and memory management.
2.  **Idiomatic API**: Functions and classes should demonstrate the best practices of Miri programming (e.g., immutability by default, trait usage).
3.  **Automatic Discovery**: The compiler automatically knows how to resolve and parse `use system.*` imports directly from this bundled module.
