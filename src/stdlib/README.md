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
-   **`system.range`**: The `Range<T>` struct range syntax (`a..b`, `a..=b`) builds, with the public fields `start`, `end` and `inclusive`.
-   **Collections (Planned)**: Modules for `List`, `Map`, `Set`, `Tuple` etc., implemented using runtime intrinsics.
-   **Math (Planned)**: Standard mathematical constants and functions (`sin`, `cos`, `abs`, etc.).

## Design Principles

1.  **Miri Native**: The standard library is overwhelmingly written in Miri, leveraging the compiler's own type system and memory management.
2.  **Idiomatic API**: Functions and classes should demonstrate the best practices of Miri programming (e.g., immutability by default, trait usage).
3.  **Automatic Discovery**: The compiler automatically knows how to resolve and parse `use system.*` imports directly from this bundled module.

## The Implicit Prelude

Four files under `system/` decide what a program gets without writing a `use`. Each lists only imports. The compiler names no stdlib module; it knows by name only the language items syntax binds to — the `Range` range syntax builds and the `Option` that `T?` stands for.

-   **`prelude.mi`**: re-exported by name and **reserved** — the rest of the stdlib is written against these types (`system.io`, `system.string`), and range syntax builds the `Range` that `system.range` declares, so a program cannot declare its own type of the same name.
-   **`prelude_shadowable.mi`**: re-exported by name but **skipped** for any module whose types the program declares itself (`system.result`). A program that declares its own `Result` keeps it, and importing a module that needs the stdlib one is reported as a conflict at the import.
-   **`prelude_internal.mi`**: loaded for definitions only, so collection and tuple literals resolve their methods while naming `Array`/`List`/`Map`/`Set`/`Tuple` still needs an explicit `use system.collections.*`.
-   **`prelude_regex.mi`**: loaded for definitions only, and only for a file that writes a regex literal (`re"..."`) or a regex match pattern. `system.text` declares the class that syntax is built through and tested with, found by its `@regex_literal` and `@regex_match` methods, so the compiler names no regex class. Naming `Regex` still needs `use system.text`. A program without regex syntax never loads it, so its own `Result` stays declarable; one that writes regex syntax and declares its own `Result` is told of the conflict.

A module belongs in the shadowable tier only when nothing else in the stdlib depends on the types it declares.
