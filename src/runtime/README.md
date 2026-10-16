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
4.  **One exit for a fault**: A program that dies of a runtime fault leaves through `core/src/trap.rs`, which takes the diagnostic code as an argument. Reporting the code is therefore part of dying, and a trap cannot reach a reader as a sentence on stderr beside an envelope that says the run succeeded.

## Counters for diagnosing memory

Two environment variables make a compiled program report what its memory did,
on stderr at exit:

- `MIRI_ALLOC_COUNT=1` counts allocations by kind: reference-counted values
  (`rc`), inline aggregates (`inline`), collection buffers (`buffers`) and raw
  blocks (`raw`).
- `MIRI_RC_STATS=1` counts reference-counting work: the increments (`dup`) and
  decrements (`drop`) compiled code made, the decrements that released a value
  (`drop_free`), the copy-on-write writes that copied a shared collection
  (`cow_copy`), and the buffers the runtime grew (`realloc`).

Compiled code reaches the counters through hooks it guards with one runtime
byte, the tracking state the heap guard and leak check also use. With every
observer off an allocation, an increment or a release pays one load and one
predicted branch, and no call. Reference-counting work the runtime does on
its own behalf, such as releasing a map's keys or a collection's elements, is
not counted. So on a program whose values compiled code releases, the counts
reconcile under the leak check, with `drop_free` equal to `rc + inline` (as on
binary-trees). A program that keeps values in collections shows fewer.

The same byte decides who answers `miri_rt_rc_is_shared`, which a write into a
field of a list element or a struct asks before it writes. With every
observer off, compiled code reads the count word itself and compares it with
1; with the heap guard or a counter on, it calls the runtime, so the guard
still checks that the block is live. The runtime settles the byte at load
time, so the inline answer applies from the first write.
