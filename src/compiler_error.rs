// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::syntax_error::{find_line_info, SyntaxError};
use crate::type_error::TypeError;
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
                let (line_num, col_num, line_str) = find_line_info(source, e.span.start);
                let len = if e.span.end > e.span.start {
                    e.span.end - e.span.start
                } else {
                    1
                };
                let underline = "^".repeat(len);
                format!(
                    "Type Error: {}\n\
                      --> line {}:{}\n\
                       |\n\
                       | {}\n\
                       | {}{}\n",
                    e.message,
                    line_num,
                    col_num,
                    line_str,
                    " ".repeat(col_num - 1),
                    underline
                )
            }
            CompilerError::TypeErrors(errs) => errs
                .iter()
                .map(|e| {
                    let (line_num, col_num, line_str) = find_line_info(source, e.span.start);
                    let len = if e.span.end > e.span.start {
                        e.span.end - e.span.start
                    } else {
                        1
                    };
                    let underline = "^".repeat(len);
                    format!(
                        "Type Error: {}\n\
                          --> line {}:{}\n\
                           |\n\
                           | {}\n\
                           | {}{}\n",
                        e.message,
                        line_num,
                        col_num,
                        line_str,
                        " ".repeat(col_num - 1),
                        underline
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => format!("{}", self),
        }
    }
}
