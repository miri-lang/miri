# Codegen

The `codegen` module is responsible for the final phase of compilation: **translating the optimized Mid-Level Intermediate Representation (MIR) into executable machine code.**

## Overview

Miri's codegen architecture is designed around pluggable backends. The current implementation uses [Cranelift](https://github.com/bytecodealliance/wasmtime/tree/main/cranelift) to emit native binaries.

The code generation phase maps MIR constructs directly into target-specific instructions. It handles memory allocation (stack and heap), function calling conventions, literal initialization (like strings), and native control flow bridging.

## Architecture

-   **Backend Traits**: The system is abstracted behind common traits to support multiple generation targets (e.g., Cranelift, LLVM, SPIR-V, CUDA, WebAssembly, etc.).
-   **Translator (`translator.rs`)**: The core loop that visits MIR Basic Blocks and issues backend API calls.
-   **Variables and Locals**: MIR locals (`_0`, `_1`, etc.) are mapped to backend stack slots or virtual registers.
-   **Function Declarations**: Miri functions are exported with defined signatures (`declare_func_in_func`).
-   **Runtime Execution**: The codegen automatically links to the `miri_rt` runtime for features requiring heap allocation (e.g., strings) or complex operations (I/O).
-   **Source locations**: Asked for a code map (`CraneliftOptions::code_map`, which `--emit lines` and `--profile` set), the translator marks each statement's and terminator's instructions with the byte offset its span starts at; a span covering no text is synthesized and keeps the location before it. After each user function is defined, the runs Cranelift recorded are returned in `CompiledArtifact::code_map` as backend-neutral `FunctionCodeMap`s: the link symbol, the code size, the body's `SourceId` and `(start, end, source offset)` runs. Codegen never reads source text; the pipeline turns offsets into lines. The locations change none of the machine code, and a build that does not ask sets none.
-   **Closure trampolines (`cranelift/closure_trampolines.rs`)**: A runtime function that calls a Miri closure is handed it together with the address of a trampoline (see the closure ABI in `src/mir/README.md`). Before any body is compiled, every trampoline the runtime calls name is emitted once, keyed by its shape: it loads the closure's code pointer, loads each argument the closure takes as a word out of the slot address it is handed or passes that address on, calls the closure and stores the result through the result address. Each closure is checked against its trampoline first — the signature the trampoline calls through must be the one the closure is compiled under — and a mismatch is a compile error. Every CPU backend must define each trampoline the bodies it compiles reference: the MIR names them as function operands, and nothing else emits them.

## Design Principles

1.  **Zero-Cost Abstractions**: The codegen aims to translate MIR directly to optimized machine code, keeping overhead minimal.
2.  **Safety via Runtime**: Safe constructs like strings and arrays are backed by a lean runtime library, dynamically linked or statically compiled.
3.  **Explicit Memory Management**: Generates all necessary drops/deallocations injected during MIR optimizations (Perceus RC).
