# Standard Library (Stdlib)

The `stdlib` module contains the **Miri Standard Library**, written in Miri itself.

## Overview

The Standard Library provides the built-in functionality available to all Miri programs. It sits on top of the generic `runtime` module, offering a rich, type-safe API for common programming tasks.

## Architecture

-   **`system.io`**: Utilities for reading and writing to standard input, standard output, and standard error (e.g., `print`, `println`).
-   **`system.fs`**: Filesystem operations via the `Fs` capability class (reading, writing, listing directories, etc.).
-   **`system.os`**: Environment variables and command-line arguments via `Env` and `Args` classes, plus platform information.
-   **`system.process`**: Process control (e.g., `exit(code)`).
-   **`system.string`**: The core `String` class, extending the runtime representation with methods for concatenation, comparison, and formatting.
-   **`system.result`**: The `Result<T, E>` enum returned by every fallible stdlib API.
-   **Collections (Planned)**: Modules for `List`, `Map`, `Set`, `Tuple` etc., implemented using runtime intrinsics.
-   **Math (Planned)**: Standard mathematical constants and functions (`sin`, `cos`, `abs`, etc.).

## Design Principles

1.  **Miri Native**: The standard library is overwhelmingly written in Miri, leveraging the compiler's own type system and memory management.
2.  **Idiomatic API**: Functions and classes should demonstrate the best practices of Miri programming (e.g., immutability by default, trait usage).
3.  **Automatic Discovery**: The compiler automatically knows how to resolve and parse `use system.*` imports directly from this bundled module.

## The Implicit Prelude

Three files under `system/` decide what every program gets without writing a `use`. Each lists only imports, so the compiler hardcodes no stdlib names.

-   **`prelude.mi`**: re-exported by name and **reserved** — the rest of the stdlib is written against these types (`system.io`, `system.string`), so a program cannot declare its own type of the same name.
-   **`prelude_shadowable.mi`**: re-exported by name but **skipped** for any module whose types the program declares itself (`system.result`). A program that declares its own `Result` keeps it, and importing a module that needs the stdlib one is reported as a conflict at the import.
-   **`prelude_internal.mi`**: loaded for definitions only, so collection literals resolve their methods while naming `Array`/`List`/`Map`/`Set` still needs an explicit `use system.collections.*`.

A module belongs in the shadowable tier only when nothing else in the stdlib depends on the types it declares.
