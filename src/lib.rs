// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

pub mod cli;
pub mod version;
pub mod lexer;
pub mod syntax_error;
pub mod parser;
pub mod ast;
pub mod ast_factory;
pub mod type_checker;
pub mod type_error;
pub mod repl;
pub mod compiler_error;
pub mod pipeline;
