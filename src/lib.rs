// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

/// Abstract Syntax Tree types and construction utilities.
pub mod ast;
/// Command-line interface (argument parsing, REPL, version info).
pub mod cli;
/// Code generation backends (Cranelift, LLVM).
pub mod codegen;
/// Error and diagnostic types for all compiler phases.
pub mod error;
/// Lexer (tokenizer) for Miri source code.
pub mod lexer;
/// Mid-level Intermediate Representation (MIR).
pub mod mir;
/// Parser that produces an AST from a token stream.
pub mod parser;
/// Compilation pipeline orchestrating all phases.
pub mod pipeline;
/// Type checker and inference engine.
pub mod type_checker;
