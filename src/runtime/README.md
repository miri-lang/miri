# Runtime

The `runtime` module contains the **built-in functions and execution support infrastructure** for Miri programs.

## Overview

While simple Miri programs compile directly to native CPU instructions, more complex features (like string manipulation, I/O, dynamic collections) require a supporting runtime. This module defines the definitions that bridge compiled Miri code and host system capabilities.

Currently, this module sets up the scaffolding for interacting with standard operating system APIs (like allocating strings via the system allocator, writing to `stdout`, etc.).

## Architecture

-   **Core Intrinsics**: Functions that expose bare-metal capabilities (allocation, raw pointers, math operations) directly to the Miri programmer.
-   **Standard Objects**: Implementations of the foundational types that standard libraries build upon (e.g., the base representation of `String`, Lists, Maps).
-   **IO Operations**: Native bindings to file system and standard streams.

## Design Principles

1.  **Minimal Overhead**: The runtime must introduce as little overhead as possible, avoiding heavy garbage collectors in flavor of reference counting (Perceus RC).
2.  **Interoperability**: The runtime functions expose a C ABI, allowing them to be linked effortlessly by Cranelift or LLVM backends.
3.  **Safety**: The runtime provides safe wrappers around low-level operations (like bounding list accesses).
