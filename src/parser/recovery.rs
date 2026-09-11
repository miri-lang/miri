// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Recognising the point a failed parse can start again from.
//!
//! Syntax is the most common way generated code fails, and a parse that stops
//! at the first fault costs one invocation per fault. Resuming at the next
//! top-level declaration lets a single parse reject several of them.

use crate::lexer::Token;

/// The tokens a top-level declaration or statement can open with.
///
/// It covers every token the statement dispatcher branches on except `Indent`,
/// which opens a nested block and therefore sits inside the declaration being
/// abandoned; a gate below fails the build if the two drift apart. It also
/// holds `@`, which opens an attribute the dispatcher reads before its match,
/// and `match`, which reaches the dispatcher as an expression statement.
///
/// Tokens that continue a construct already open — `else`, `in`, `extends` —
/// are deliberately absent: they name a position inside the declaration being
/// abandoned rather than the start of the next one.
const DECLARATION_OPENERS: &[Token] = &[
    Token::At,
    Token::Use,
    Token::Fn,
    Token::Async,
    Token::Parallel,
    Token::Gpu,
    Token::Runtime,
    Token::Intrinsic,
    Token::Class,
    Token::Struct,
    Token::Enum,
    Token::Trait,
    Token::Type,
    Token::Let,
    Token::Var,
    Token::Const,
    Token::Shared,
    Token::Public,
    Token::Private,
    Token::Protected,
    Token::Abstract,
    Token::MustUse,
    Token::If,
    Token::Unless,
    Token::While,
    Token::Until,
    Token::Do,
    Token::For,
    Token::Forall,
    Token::Forever,
    Token::Match,
    Token::Return,
    Token::Break,
    Token::Continue,
];

/// Whether `token` can open a top-level declaration.
pub(super) fn opens_a_declaration(token: &Token) -> bool {
    DECLARATION_OPENERS.contains(token)
}

/// Whether the token starting at `offset` is the first thing on its line.
///
/// Column zero is what makes a declaration a resume point: every line inside a
/// declaration is indented under it, so a line that is not indented cannot
/// belong to the declaration being abandoned.
pub(super) fn opens_a_line(source: &str, offset: usize) -> bool {
    offset == 0 || source.as_bytes().get(offset.wrapping_sub(1)) == Some(&b'\n')
}

/// The source of the statement dispatcher, read by the drift gate below.
#[cfg(test)]
const STATEMENT_DISPATCH_SOURCE: &str = include_str!("statements.rs");

#[cfg(test)]
mod tests {
    use super::*;

    /// Tokens `dispatch_statement` branches on that are deliberately not resume
    /// points, with the reason each is excluded.
    const NOT_A_RESUME_POINT: &[(&str, &str)] = &[(
        "Indent",
        "opens a nested block, so it is inside the declaration being abandoned",
    )];

    /// The body of `dispatch_statement`, sliced out of the parser's source.
    fn statement_dispatch_body() -> &'static str {
        let source = STATEMENT_DISPATCH_SOURCE;
        let start = source
            .find("fn dispatch_statement")
            .expect("the statement dispatcher must exist");
        let body = &source[start..];
        let end = body
            .find("\n    }\n")
            .expect("the statement dispatcher must be a closed function");
        &body[..end]
    }

    /// Every token named in a `Some((Token::X, ...))` pattern in `body`.
    fn tokens_dispatched_on(body: &str) -> Vec<&str> {
        let mut names = Vec::new();
        for (offset, _) in body.match_indices("Some((Token::") {
            let rest = &body[offset + "Some((Token::".len()..];
            let end = rest
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            let name = &rest[..end];
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// The gate that keeps resynchronisation from silently going stale.
    ///
    /// A keyword added to the statement dispatcher and not to the resume set
    /// would leave the parser unable to restart at declarations written with
    /// it — reporting one fault per invocation again, with nothing failing.
    /// Excluding one is a decision, made here, with its reason written down.
    #[test]
    fn every_dispatched_statement_keyword_is_a_resume_point_or_is_excused() {
        let body = statement_dispatch_body();
        let dispatched = tokens_dispatched_on(body);
        assert!(
            dispatched.len() > 20,
            "the slice of the dispatcher found only {} tokens, so the gate is \
             reading the wrong text",
            dispatched.len()
        );

        for name in dispatched {
            if NOT_A_RESUME_POINT
                .iter()
                .any(|(excused, _)| *excused == name)
            {
                continue;
            }
            assert!(
                DECLARATION_OPENERS
                    .iter()
                    .any(|token| format!("{:?}", token) == name),
                "`{}` opens a statement but is not a resume point: a file whose \
                 declarations are written with it reports one fault per \
                 invocation. Add it to DECLARATION_OPENERS, or to \
                 NOT_A_RESUME_POINT with the reason it cannot be one.",
                name
            );
        }
    }

    #[test]
    fn an_excused_token_is_still_dispatched_on() {
        let dispatched = tokens_dispatched_on(statement_dispatch_body());
        for (excused, _) in NOT_A_RESUME_POINT {
            assert!(
                dispatched.contains(excused),
                "`{}` is excused from the resume set but the dispatcher no \
                 longer branches on it; drop the exclusion",
                excused
            );
        }
    }

    #[test]
    fn declaration_keywords_open_a_declaration() {
        assert!(opens_a_declaration(&Token::Fn));
        assert!(opens_a_declaration(&Token::Class));
        assert!(opens_a_declaration(&Token::At));
    }

    #[test]
    fn a_continuation_keyword_does_not_open_a_declaration() {
        assert!(!opens_a_declaration(&Token::Else));
        assert!(!opens_a_declaration(&Token::In));
        assert!(!opens_a_declaration(&Token::Extends));
        assert!(!opens_a_declaration(&Token::Identifier));
    }

    #[test]
    fn the_first_byte_of_the_file_opens_a_line() {
        assert!(opens_a_line("fn a()\n", 0));
    }

    #[test]
    fn a_byte_after_a_newline_opens_a_line() {
        assert!(opens_a_line("fn a()\nfn b()\n", 7));
    }

    #[test]
    fn an_indented_byte_does_not_open_a_line() {
        assert!(!opens_a_line("fn a()\n    return 1\n", 11));
        assert!(!opens_a_line("fn a()\n", 3));
    }
}
