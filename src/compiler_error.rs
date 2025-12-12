// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use thiserror::Error;
use crate::syntax_error::SyntaxError;

#[derive(Error, Debug)]
pub enum CompilerError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Lexer Error: {0:?}")]
    Lexer(SyntaxError),

    #[error("Parser Error: {0:?}")]
    Parser(SyntaxError),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Internal compiler error: {0}")]
    Internal(String),
}

impl CompilerError {
    pub fn report(&self, source: &str) -> String {
        match self {
            CompilerError::Lexer(e) | CompilerError::Parser(e) => e.report(source),
            _ => format!("{}", self),
        }
    }
}
