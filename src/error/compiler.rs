// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::error::syntax::SyntaxError;
use crate::error::type_error::TypeError;
use crate::error::utils::format_diagnostic;
use thiserror::Error;

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
}

impl CompilerError {
    pub fn report(&self, source: &str) -> String {
        match self {
            CompilerError::Lexer(e) | CompilerError::Parser(e) => e.report(source),
            CompilerError::Type(e) => {
                format_diagnostic(source, &e.span, &e.message, "error", e.help.as_deref())
            }
            CompilerError::TypeErrors(errs) => errs
                .iter()
                .map(|e| format_diagnostic(source, &e.span, &e.message, "error", e.help.as_deref()))
                .collect::<Vec<_>>()
                .join("\n"),
            _ => format!("{}", self),
        }
    }
}
