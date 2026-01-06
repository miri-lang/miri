// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::ast::expression::Expression;
use crate::ast::literal::Literal;
use crate::ast::statement::Statement;
use crate::lexer::RegexToken;

/// Represents a branch in a match expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MatchBranch {
    pub patterns: Vec<Pattern>,
    pub guard: Option<Box<Expression>>,
    pub body: Box<Statement>,
}

/// Represents a pattern in a match expression
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Pattern {
    Literal(Literal),
    Identifier(String),
    Tuple(Vec<Pattern>),
    Regex(RegexToken),
    Default,
    Member(Box<Pattern>, String),
    /// Enum variant with bindings: Color.Red(x, y)
    /// First is the enum path (e.g., Color.Red), second is the binding patterns
    EnumVariant(Box<Pattern>, Vec<Pattern>),
}
