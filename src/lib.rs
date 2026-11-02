// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

/// The memory layout compiled code and the runtime agree on.
pub mod abi;
/// Abstract Syntax Tree types and construction utilities.
pub mod ast;
/// Command-line interface (argument parsing, REPL, version info).
pub mod cli;
/// Code generation backends (Cranelift, LLVM).
pub mod codegen;
/// Diagnostic code registry and severity levels.
pub mod diagnostics;
/// Error and diagnostic types for all compiler phases.
pub mod error;
/// GPU-target facts shared by the type checker, MIR lowering and code generation.
pub mod gpu_target;
/// Lexer (tokenizer) for Miri source code.
pub mod lexer;
/// Mid-level Intermediate Representation (MIR).
pub mod mir;
/// Parser that produces an AST from a token stream.
pub mod parser;
/// Compilation pipeline orchestrating all phases.
pub mod pipeline;
/// What each runtime entry point does to its arguments and its result.
pub mod runtime_effects;
/// Runtime function name constants (`miri_rt_*` symbols).
pub mod runtime_fns;
/// Test discovery and execution for `@test` functions.
pub mod test_runner;
/// Type checker and inference engine.
pub mod type_checker;
