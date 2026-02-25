# Mid-Level Intermediate Representation (MIR)

The `mir` module defines the Mid-Level Intermediate Representation, a Control-Flow Graph (CFG) based representation of Miri programs used for advanced analysis and optimization.

## Overview

While the Abstract Syntax Tree (AST) represents the syntactic structure of code, it implicitly encodes control flow (like loops and branches). MIR flattens this structure into explicit jumps between Basic Blocks, making dataflow analysis, optimization, and code generation much simpler.

## Core Concepts

-   **Basic Block (BB)**: A linear sequence of statements with a single entry point and a single exit point (a `Terminator`).
-   **Terminator**: The instruction at the end of a block that dictates control flow (e.g., `Goto`, `SwitchInt`, `Return`, `Call`).
-   **Locals (`_0`, `_1`, etc.)**: All variables and temporaries are flattened into an indexed array of Locals. `_0` is always the return value.
-   **Places and Rvalues**: Assignments take the form of `Place = Rvalue`, meaning a computational result (`Rvalue`) is written into a memory location (`Place`).

## Architecture

-   **Lowering (`src/mir/lowering/`)**: Translates the checked AST into the MIR format.
-   **Optimization (`src/mir/optimization/`)**: A suite of passes (`SimplifyCfg`, `ConstantPropagation`, `CopyPropagation`, `DeadCodeElimination`, `Perceus RC`) that transform and optimize the MIR inplace.
-   **SSA Form (`src/mir/ssa/`)**: Infrastructure to convert MIR into Static Single Assignment form (using Phi nodes) for advanced analysis, and back out of it.
-   **Dominator Analysis**: Computes dominator trees used by SSA and optimization passes.
-   **GPU Metadata (`backend/`)**: Extensions to the MIR to support heterogeneous execution models, handling GPU kernel limits, barriers, and thread indices.

## Design Principles

1.  **Explicit Control Flow**: All loops and conditionals are desugared into simple conditional and unconditional branches.
2.  **Visitor Pattern**: Extensively utilizes immutable and mutable Visitor traits to traverse and transform the MIR graph safely.
3.  **Backend Agnostic**: The MIR is optimized specifically for Miri's semantics but remains decoupled from the final target architecture (Cranelift, LLVM, SPIR-V).
