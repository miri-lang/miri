// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::statement::Statement;

/// Represents a fully parsed Miri program
#[derive(Debug, PartialEq)]
pub struct Program {
    pub body: Vec<Statement>,
}
