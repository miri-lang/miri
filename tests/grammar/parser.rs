// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A minimal packrat PEG interpreter for parsing `docs/grammar.peg` and matching token streams.

use std::collections::HashMap;
use std::fmt;

use miri::lexer::terminal::{classify_token, TerminalClassification};
use miri::lexer::Token;

/// Errors during grammar parsing or matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PegError {
    /// Failed to parse the grammar file.
    GrammarParseFailed(String),
    /// Rule not found in the grammar.
    RuleNotFound(String),
    /// Token matching failed at position.
    MatchFailed { rule: String, pos: usize },
    /// Invalid grammar syntax.
    InvalidGrammar(String),
}

impl fmt::Display for PegError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GrammarParseFailed(msg) => write!(f, "grammar parse failed: {}", msg),
            Self::RuleNotFound(name) => write!(f, "rule not found: {}", name),
            Self::MatchFailed { rule, pos } => {
                write!(f, "match failed at rule '{}' pos {}", rule, pos)
            }
            Self::InvalidGrammar(msg) => write!(f, "invalid grammar: {}", msg),
        }
    }
}

impl std::error::Error for PegError {}

/// Match result: (consumed_count, success)
type MatchResult = (usize, bool);

/// A PEG matcher with memoization.
/// Bound on nested rule applications, guarding against a left-recursive grammar.
const MAX_MATCH_DEPTH: usize = 1024;

pub struct PegMatcher {
    grammar: PegGrammar,
    memo: HashMap<(String, usize), MatchResult>,
    depth: usize,
    /// Farthest token position any alternative reached. PEG backtracking reports
    /// a failure at the start of the construct it gave up on, which is rarely
    /// where the grammar is actually wrong; the farthest position is.
    farthest: usize,
}

impl PegMatcher {
    /// Creates a new matcher for a grammar.
    pub fn new(grammar: PegGrammar) -> Self {
        PegMatcher {
            grammar,
            memo: HashMap::new(),
            depth: 0,
            farthest: 0,
        }
    }

    /// Matches tokens starting from the start rule.
    /// Returns Ok(()) if the entire token stream matches, Err(PegError) otherwise.
    pub fn match_tokens(&mut self, tokens: &[Token]) -> Result<(), PegError> {
        let start_rule = "program";
        let (consumed, success) = self.match_expr(start_rule, tokens, 0)?;

        if success && consumed == tokens.len() {
            Ok(())
        } else if success {
            Err(PegError::MatchFailed {
                rule: start_rule.to_string(),
                pos: consumed,
            })
        } else {
            Err(PegError::MatchFailed {
                rule: start_rule.to_string(),
                pos: 0,
            })
        }
    }

    /// Farthest token position reached during the last match attempt.
    pub fn farthest(&self) -> usize {
        self.farthest
    }

    fn match_expr(
        &mut self,
        rule_name: &str,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        if pos > tokens.len() {
            return Ok((pos, false));
        }

        let memo_key = (rule_name.to_string(), pos);
        if let Some(&result) = self.memo.get(&memo_key) {
            return Ok(result);
        }

        if let Some(expr) = self.grammar.rules.get(rule_name).cloned() {
            // A left-recursive rule re-enters itself at the same position before any
            // memo entry exists, which would recurse until the native stack is gone.
            // The bound turns that into a readable failure.
            if self.depth >= MAX_MATCH_DEPTH {
                return Err(PegError::InvalidGrammar(format!(
                    "rule `{rule_name}` recursed past {MAX_MATCH_DEPTH} frames; \
                     the grammar is probably left-recursive"
                )));
            }
            self.depth += 1;
            let result = self.match_peg_expr(&expr, tokens, pos);
            self.depth -= 1;
            let result = result?;
            self.memo.insert(memo_key, result);
            Ok(result)
        } else {
            // If it's not a rule, treat it as a terminal token name
            let result = self.match_literal(rule_name, tokens, pos)?;
            self.memo.insert(memo_key, result);
            Ok(result)
        }
    }

    fn match_peg_expr(
        &mut self,
        expr: &PegExpr,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        if pos > tokens.len() {
            return Ok((pos, false));
        }

        match expr {
            PegExpr::RuleRef(name) => self.match_expr(name, tokens, pos),
            PegExpr::Literal(literal) => self.match_literal(literal, tokens, pos),
            PegExpr::Sequence(exprs) => self.match_sequence(exprs, tokens, pos),
            PegExpr::Choice(exprs) => self.match_choice(exprs, tokens, pos),
            PegExpr::Star(inner) => self.match_star(inner, tokens, pos),
            PegExpr::Plus(inner) => self.match_plus(inner, tokens, pos),
            PegExpr::Optional(inner) => self.match_optional(inner, tokens, pos),
            PegExpr::And(inner) => self.match_and(inner, tokens, pos),
            PegExpr::Not(inner) => self.match_not(inner, tokens, pos),
            PegExpr::Group(inner) => self.match_peg_expr(inner, tokens, pos),
        }
    }

    fn match_literal(
        &mut self,
        literal: &str,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        if pos >= tokens.len() {
            return Ok((pos, false));
        }

        self.farthest = self.farthest.max(pos);
        let success = match self.token_name(&tokens[pos]) {
            Some(name) => name == literal,
            None => false,
        };

        if success {
            Ok((pos + 1, true))
        } else {
            Ok((pos, false))
        }
    }

    fn match_sequence(
        &mut self,
        exprs: &[PegExpr],
        tokens: &[Token],
        mut pos: usize,
    ) -> Result<MatchResult, PegError> {
        for expr in exprs {
            let (new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
            if !success {
                return Ok((pos, false));
            }
            pos = new_pos;
        }
        Ok((pos, true))
    }

    fn match_choice(
        &mut self,
        exprs: &[PegExpr],
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        for expr in exprs {
            let (new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
            if success {
                return Ok((new_pos, true));
            }
        }
        Ok((pos, false))
    }

    fn match_star(
        &mut self,
        expr: &PegExpr,
        tokens: &[Token],
        mut pos: usize,
    ) -> Result<MatchResult, PegError> {
        loop {
            let (new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
            if !success {
                break;
            }
            // A repetition whose body consumes nothing would spin forever; one
            // zero-width success is all the repetition can contribute.
            if new_pos == pos {
                break;
            }
            pos = new_pos;
        }
        Ok((pos, true))
    }

    fn match_plus(
        &mut self,
        expr: &PegExpr,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        let (new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
        if !success {
            return Ok((pos, false));
        }

        let mut pos = new_pos;
        loop {
            let (new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
            if !success {
                break;
            }
            // A repetition whose body consumes nothing would spin forever; one
            // zero-width success is all the repetition can contribute.
            if new_pos == pos {
                break;
            }
            pos = new_pos;
        }
        Ok((pos, true))
    }

    fn match_optional(
        &mut self,
        expr: &PegExpr,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        let (new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
        if success {
            Ok((new_pos, true))
        } else {
            Ok((pos, true))
        }
    }

    fn match_and(
        &mut self,
        expr: &PegExpr,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        let (_new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
        Ok((pos, success))
    }

    fn match_not(
        &mut self,
        expr: &PegExpr,
        tokens: &[Token],
        pos: usize,
    ) -> Result<MatchResult, PegError> {
        let (_new_pos, success) = self.match_peg_expr(expr, tokens, pos)?;
        Ok((pos, !success))
    }

    /// Resolves a token to its published grammar terminal name.
    ///
    /// Delegates to `classify_token` in the lexer so the grammar and the lexer
    /// share one mapping. A token that is not a grammar terminal has no name and
    /// therefore matches no terminal in the grammar.
    fn token_name(&self, token: &Token) -> Option<String> {
        match classify_token(token) {
            TerminalClassification::Terminal(name) => Some(name),
            TerminalClassification::NotTerminal(_) => None,
        }
    }
}

/// A PEG expression AST node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PegExpr {
    /// Rule reference: `name`
    RuleRef(String),
    /// Literal string: `"foo"`
    Literal(String),
    /// Sequence: `a b c`
    Sequence(Vec<PegExpr>),
    /// Choice: `a / b / c`
    Choice(Vec<PegExpr>),
    /// Zero or more: `a*`
    Star(Box<PegExpr>),
    /// One or more: `a+`
    Plus(Box<PegExpr>),
    /// Optional: `a?`
    Optional(Box<PegExpr>),
    /// Positive lookahead: `&a`
    And(Box<PegExpr>),
    /// Negative lookahead: `!a`
    Not(Box<PegExpr>),
    /// Grouping: `(a b / c)`
    Group(Box<PegExpr>),
}

/// A PEG rule: `name <- expr`
#[derive(Debug, Clone)]
pub struct PegRule {
    pub name: String,
    pub expr: PegExpr,
}

/// A parsed PEG grammar.
#[derive(Debug, Clone)]
pub struct PegGrammar {
    pub version: Option<String>,
    pub rules: HashMap<String, PegExpr>,
}

impl PegGrammar {
    /// Parses a PEG grammar from a string.
    pub fn parse(input: &str) -> Result<Self, PegError> {
        let mut grammar = PegGrammar {
            version: None,
            rules: HashMap::new(),
        };

        let mut lines = input.lines().peekable();

        // Parse version line if present
        while let Some(line) = lines.peek() {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                if trimmed.starts_with("# version:") {
                    let parts: Vec<&str> = trimmed.split(':').collect();
                    if parts.len() == 2 {
                        grammar.version = Some(parts[1].trim().to_string());
                    }
                }
                lines.next();
            } else if trimmed.is_empty() {
                lines.next();
            } else {
                break;
            }
        }

        // Parse rules
        let remaining = lines.collect::<Vec<_>>().join("\n");
        let rules = Self::parse_rules(&remaining)?;
        for rule in rules {
            grammar.rules.insert(rule.name, rule.expr);
        }

        Ok(grammar)
    }

    /// Splits the grammar text into rules.
    ///
    /// A rule starts at a line carrying the `<-` definition operator and runs until
    /// the next such line, so a rule body may be wrapped across as many lines as it
    /// needs. Blank lines and `#` comments are not part of any rule.
    fn parse_rules(input: &str) -> Result<Vec<PegRule>, PegError> {
        let mut rules = Vec::new();
        let mut current_rule = String::new();

        for line in input.lines() {
            let trimmed = line.trim();

            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Rule text is sliced by byte offset, which is only safe while every
            // byte is one character. Comments are skipped above and may say
            // anything; a rule line may not.
            if !trimmed.is_ascii() {
                return Err(PegError::InvalidGrammar(format!(
                    "rule text must be ASCII: {trimmed}"
                )));
            }

            if trimmed.contains("<-") {
                if !current_rule.is_empty() {
                    rules.push(Self::parse_single_rule(&current_rule)?);
                }
                current_rule = trimmed.to_string();
            } else if !current_rule.is_empty() {
                current_rule.push(' ');
                current_rule.push_str(trimmed);
            }
        }

        if !current_rule.is_empty() {
            rules.push(Self::parse_single_rule(&current_rule)?);
        }

        Ok(rules)
    }

    fn parse_single_rule(rule_str: &str) -> Result<PegRule, PegError> {
        if let Some(pos) = rule_str.find("<-") {
            let name = rule_str[..pos].trim().to_string();
            let body = &rule_str[pos + 2..].trim();

            let expr = Self::parse_expr(body)?;

            Ok(PegRule { name, expr })
        } else {
            Err(PegError::InvalidGrammar(
                "rule missing '<-' operator".to_string(),
            ))
        }
    }

    fn parse_expr(input: &str) -> Result<PegExpr, PegError> {
        Self::parse_choice(input)
    }

    fn parse_choice(input: &str) -> Result<PegExpr, PegError> {
        let parts = Self::split_choice(input);
        if parts.is_empty() {
            return Err(PegError::InvalidGrammar("empty choice".to_string()));
        }

        if parts.len() == 1 {
            Self::parse_sequence(parts[0])
        } else {
            let mut choices = Vec::new();
            for part in parts {
                choices.push(Self::parse_sequence(part.trim())?);
            }
            Ok(PegExpr::Choice(choices))
        }
    }

    fn split_choice(input: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut current_start = 0;
        let mut paren_depth: i32 = 0;
        let mut bracket_depth: i32 = 0;

        for (i, c) in input.char_indices() {
            match c {
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                '/' if paren_depth == 0 && bracket_depth == 0 => {
                    parts.push(&input[current_start..i]);
                    current_start = i + 1;
                }
                _ => {}
            }
        }

        parts.push(&input[current_start..]);
        parts
    }

    fn parse_sequence(input: &str) -> Result<PegExpr, PegError> {
        let parts = Self::split_sequence(input);
        if parts.is_empty() {
            return Err(PegError::InvalidGrammar("empty sequence".to_string()));
        }

        if parts.len() == 1 {
            Self::parse_postfix(parts[0].trim())
        } else {
            let mut sequence = Vec::new();
            for part in parts {
                sequence.push(Self::parse_postfix(part.trim())?);
            }
            Ok(PegExpr::Sequence(sequence))
        }
    }

    fn split_sequence(input: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut current_start = 0;
        let mut paren_depth: i32 = 0;
        let mut bracket_depth: i32 = 0;
        let mut in_string = false;
        let mut prev_was_escape = false;

        for (i, c) in input.char_indices() {
            match c {
                '"' if !prev_was_escape => in_string = !in_string,
                _ if in_string => {
                    prev_was_escape = c == '\\';
                    continue;
                }
                '(' => paren_depth += 1,
                ')' => paren_depth = paren_depth.saturating_sub(1),
                '[' => bracket_depth += 1,
                ']' => bracket_depth = bracket_depth.saturating_sub(1),
                c if c.is_whitespace() && paren_depth == 0 && bracket_depth == 0 => {
                    if current_start < i {
                        parts.push(&input[current_start..i]);
                    }
                    current_start = i + 1;
                }
                _ => {}
            }
            prev_was_escape = c == '\\';
        }

        if current_start < input.len() {
            parts.push(&input[current_start..]);
        }

        parts.into_iter().filter(|p| !p.is_empty()).collect()
    }

    fn parse_postfix(input: &str) -> Result<PegExpr, PegError> {
        if input.is_empty() {
            return Err(PegError::InvalidGrammar("empty expression".to_string()));
        }

        if let Some(c) = input.chars().last() {
            let expr = if matches!(c, '*' | '+' | '?') {
                Self::parse_prefix(&input[..input.len() - 1])?
            } else {
                Self::parse_prefix(input)?
            };

            if let Some(c) = input.chars().last() {
                Ok(match c {
                    '*' => PegExpr::Star(Box::new(expr)),
                    '+' => PegExpr::Plus(Box::new(expr)),
                    '?' => PegExpr::Optional(Box::new(expr)),
                    _ => expr,
                })
            } else {
                Ok(expr)
            }
        } else {
            Err(PegError::InvalidGrammar("empty expression".to_string()))
        }
    }

    fn parse_prefix(input: &str) -> Result<PegExpr, PegError> {
        if input.is_empty() {
            return Err(PegError::InvalidGrammar("empty expression".to_string()));
        }

        if let Some(first) = input.chars().next() {
            match first {
                '&' => Ok(PegExpr::And(Box::new(Self::parse_primary(&input[1..])?))),
                '!' => Ok(PegExpr::Not(Box::new(Self::parse_primary(&input[1..])?))),
                _ => Self::parse_primary(input),
            }
        } else {
            Err(PegError::InvalidGrammar("empty expression".to_string()))
        }
    }

    fn parse_primary(input: &str) -> Result<PegExpr, PegError> {
        if input.is_empty() {
            return Err(PegError::InvalidGrammar("empty expression".to_string()));
        }

        if input == "." {
            Ok(PegExpr::RuleRef("ANY".to_string()))
        } else if input.starts_with('"') && input.ends_with('"') && input.len() > 1 {
            let literal = input[1..input.len() - 1].to_string();
            Ok(PegExpr::Literal(literal))
        } else if input.starts_with('(') && input.ends_with(')') && input.len() > 1 {
            let inner = &input[1..input.len() - 1];
            Ok(PegExpr::Group(Box::new(Self::parse_expr(inner)?)))
        } else if input.chars().all(|c| c.is_alphanumeric() || c == '_') {
            Ok(PegExpr::RuleRef(input.to_string()))
        } else {
            Err(PegError::InvalidGrammar(format!(
                "unrecognized expression: {}",
                input
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_rule() {
        let grammar_text = r#"# version: 1
program <- statement+
statement <- "let" IDENT
"#;

        let grammar = PegGrammar::parse(grammar_text).expect("should parse");
        assert_eq!(grammar.version, Some("1".to_string()));
        assert!(grammar.rules.contains_key("program"));
        assert!(grammar.rules.contains_key("statement"));
    }

    #[test]
    fn test_parse_choice() {
        let result = PegGrammar::parse_expr("a / b / c");
        assert!(result.is_ok());
        if let Ok(PegExpr::Choice(choices)) = result {
            assert_eq!(choices.len(), 3);
        } else {
            panic!("expected Choice");
        }
    }

    #[test]
    fn test_parse_sequence() {
        let result = PegGrammar::parse_expr("a b c");
        assert!(result.is_ok());
        if let Ok(PegExpr::Sequence(seq)) = result {
            assert_eq!(seq.len(), 3);
        } else {
            panic!("expected Sequence");
        }
    }

    #[test]
    fn test_parse_repetition() {
        let star_result = PegGrammar::parse_expr("a*");
        assert!(matches!(star_result, Ok(PegExpr::Star(_))));

        let plus_result = PegGrammar::parse_expr("a+");
        assert!(matches!(plus_result, Ok(PegExpr::Plus(_))));

        let opt_result = PegGrammar::parse_expr("a?");
        assert!(matches!(opt_result, Ok(PegExpr::Optional(_))));
    }
}
