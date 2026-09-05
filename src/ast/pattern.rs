// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::Expression;
use crate::ast::literal::Literal;
use crate::ast::statement::Statement;
use crate::error::syntax::Span;
use crate::lexer::RegexToken;

/// Represents a branch in a match expression
#[derive(Debug, Clone)]
pub struct MatchBranch {
    pub patterns: Vec<Pattern>,
    /// Source range of each entry in `patterns`, in the same order, so a
    /// diagnostic about a pattern can point at the pattern the author wrote
    /// rather than at the whole `match`. Empty for branches built
    /// programmatically, which have no source text to point at, and shorter
    /// than `patterns` only in that case.
    pub pattern_spans: Vec<Span>,
    pub guard: Option<Box<Expression>>,
    pub body: Box<Statement>,
    /// Whether pattern bindings in this branch should be mutable (`var` vs `let`).
    pub is_mutable: bool,
}

impl MatchBranch {
    /// The source range of the pattern at `index`, when one was recorded.
    pub fn pattern_span(&self, index: usize) -> Option<Span> {
        self.pattern_spans
            .get(index)
            .copied()
            .filter(|span| !span.is_empty())
    }
}

/// Equality and hashing ignore `pattern_spans`, matching
/// [`crate::ast::statement::FunctionDeclarationData`], where a node's source
/// location is metadata rather than part of its identity. This keeps a branch
/// built by the AST factory equal to the same branch produced by the parser.
impl PartialEq for MatchBranch {
    fn eq(&self, other: &Self) -> bool {
        self.patterns == other.patterns
            && self.guard == other.guard
            && self.body == other.body
            && self.is_mutable == other.is_mutable
    }
}

impl Eq for MatchBranch {}

impl std::hash::Hash for MatchBranch {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.patterns.hash(state);
        self.guard.hash(state);
        self.body.hash(state);
        self.is_mutable.hash(state);
    }
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
