# Codegen

The `codegen` module is responsible for the final phase of compilation: **translating the optimized Mid-Level Intermediate Representation (MIR) into executable machine code.**

## Overview

Miri's codegen architecture is designed around pluggable backends. The current implementation uses [Cranelift](https://github.com/bytecodealliance/wasmtime/tree/main/cranelift) to emit native binaries.

The code generation phase maps MIR constructs directly into target-specific instructions. It handles memory allocation (stack and heap), function calling conventions, literal initialization (like strings), and native control flow bridging.

## Architecture

-   **Backend Traits**: The system is abstracted behind common traits to support multiple generation targets (e.g., Cranelift, LLVM, SPIR-V).
-   **Translator (`translator.rs`)**: The core loop that visits MIR Basic Blocks and issues backend API calls.
-   **Variables and Locals**: MIR locals (`_0`, `_1`, etc.) are mapped to backend stack slots or virtual registers.
-   **Function Declarations**: Miri functions are exported with defined signatures (`declare_func_in_func`).
-   **Runtime Execution**: The codegen automatically links to the `miri_rt` runtime for features requiring heap allocation (e.g., strings) or complex operations (I/O).

## Design Principles

1.  **Zero-Cost Abstractions**: The codegen aims to translate MIR directly to optimized machine code, keeping overhead minimal.
2.  **Safety via Runtime**: Safe constructs like strings and arrays are backed by a lean runtime library, dynamically linked or statically compiled.
3.  **Explicit Memory Management**: Generates all necessary drops/deallocations injected during MIR optimizations (Perceus RC).
