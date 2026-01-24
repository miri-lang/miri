// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::error::lowering::LoweringError;
use crate::error::syntax::SyntaxError;
use crate::error::type_error::TypeError;
use thiserror::Error;

/// Top-level error type encompassing all compiler pipeline errors.
#[derive(Error, Debug)]
pub enum CompilerError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Lexer Error: {0:?}")]
    Lexer(SyntaxError),

    #[error("Parser Error: {0:?}")]
    Parser(SyntaxError),

    #[error("Type Error: {0}")]
    Type(TypeError),

    #[error("Type Errors: {0:?}")]
    TypeErrors(Vec<TypeError>),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Internal compiler error: {0}")]
    Internal(String),

    #[error("Codegen Error: {0}")]
    Codegen(String),

    #[error("Lowering Error: {0}")]
    Lowering(LoweringError),

    #[error("Runtime Error: {0}")]
    Runtime(String),
}

impl CompilerError {
    /// Formats this error for terminal display using the given source code.
    pub fn report(&self, source: &str) -> String {
        match self {
            CompilerError::Lexer(e) | CompilerError::Parser(e) => e.report(source),
            CompilerError::Type(e) => e.report(source),
            CompilerError::TypeErrors(errs) => errs
                .iter()
                .map(|e| e.report(source))
                .collect::<Vec<_>>()
                .join("\n"),
            CompilerError::Lowering(e) => e.report(source),
            _ => format!("{}", self),
        }
    }
}
