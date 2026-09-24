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
-   **Residency readbacks (`src/mir/residency/`)**: A required pass, run on every host body after lowering and before Perceus, that reads a `gpu`-resident binding's device buffer back into its host array before each host read that could otherwise see stale values. A forward dataflow tracks, per device handle, whether the host array may lag the device: a read every path reaches behind reads back unconditionally, a read only some paths reach behind (the first read in a loop after a launch) tests a flag the pass keeps for the handle, and a read no path reaches behind gets nothing. Each readback first gives the binding a host array of its own, so earlier host copies keep their values. `verify::verify_cross_residency_readback` independently checks the result under `--verify-mir`.
-   **Optimization (`src/mir/optimization/`)**: A suite of passes (`SimplifyCfg`, `ConstantPropagation`, `CopyPropagation`, `DeadCodeElimination`, `Perceus RC`) that transform and optimize the MIR inplace.
-   **Element ABI (`src/mir/element_abi.rs`)**: The last pass before codegen. Every call that stores or looks up a collection element (`runtime_fns::element_positions`) is lowered with the element as one argument, which is how Perceus and `--verify-mir` read it. This pass rewrites each such argument into the address of the element's bytes (`Rvalue::Ref`, or the operand itself for an inline vector) and the count of those bytes, taken from `ast::types::element_layout`. The runtime lays the element out at its slot's full width, so List, Set and Map share one convention at every element width and no backend knows which arguments are elements. A call that hands an element back (`runtime_fns::returns_element_value`) is spelled the same way in reverse: the runtime writes the element into caller storage named by two trailing arguments — address and byte count — and returns nothing. A register-held destination becomes an `out` argument, an inline vector is built with zero components and its storage handed over, and anything else goes through a register-sized slot assigned to the destination after the call.
-   **SSA Form (`src/mir/ssa/`)**: Infrastructure to convert MIR into Static Single Assignment form (using Phi nodes) for advanced analysis, and back out of it.
-   **Dominator Analysis**: Computes dominator trees used by SSA and optimization passes.
-   **GPU Metadata (`backend/`)**: Extensions to the MIR to support heterogeneous execution models, handling GPU kernel limits, barriers, and thread indices.

## Monomorphization

A generic function or generic-class method is lowered once per instantiation, with its substitution (`LoweringContext::generic_subs`) applied to every recorded type. What such a body reaches is recorded on the `Body` rather than recovered from mangled symbols:

-   `generic_function_calls` — each generic function it calls, at the types the call pins. The pipeline lowers those in a worklist.
-   `generic_class_instantiations` — each generic class it names only through its substitution (`Box<T>` inside `via_box<T>` lowered at `String`). The pipeline adds these to the type checker's instantiation registry, then emits the methods they call, repeating until nothing new is reached. Codegen reads the same registry for per-instantiation drop functions and element-method thunks.

## Design Principles

1.  **Explicit Control Flow**: All loops and conditionals are desugared into simple conditional and unconditional branches.
2.  **Visitor Pattern**: Extensively utilizes immutable and mutable Visitor traits to traverse and transform the MIR graph safely.
3.  **Backend Agnostic**: The MIR is optimized specifically for Miri's semantics but remains decoupled from the final target architecture (Cranelift, LLVM, SPIR-V).
