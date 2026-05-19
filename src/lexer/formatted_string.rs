// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::token::{Token, TokenSpan};
use crate::error::syntax::{Span, SyntaxError, SyntaxErrorKind};
use crate::lexer::Lexer;
use logos::Lexer as LogosLexer;

/// Lexes an f-string literal, splitting it into string parts and interpolated expressions.
pub fn lex_formatted_string(
    lexer: &mut LogosLexer<Token>,
    pending_tokens_stack: &mut Vec<TokenSpan>,
    quote_character: char,
) -> Result<(), SyntaxError> {
    let body_offsets = locate_body(lexer.slice(), quote_character)
        .ok_or_else(|| error_here(lexer, SyntaxErrorKind::InvalidFormattedString))?;
    let string_body = &lexer.slice()[body_offsets.start..body_offsets.end];
    // Two bytes for the leading `f"` / `f'`.
    let token_offset = lexer.span().start + 2;

    let mut tokens: Vec<TokenSpan> = Vec::new();
    let mut cursor = 0;
    let mut is_first_part = true;

    while cursor < string_body.len() {
        let Some(brace_pos) = find_next_unescaped_brace(string_body, cursor) else {
            break;
        };

        push_literal_part(
            &mut tokens,
            &string_body[cursor..brace_pos],
            Span::new(token_offset + cursor, token_offset + brace_pos),
            &mut is_first_part,
        );

        let expr_start = brace_pos + 1;
        let expr_end = find_matching_close_brace(string_body, expr_start)
            .ok_or_else(|| error_here(lexer, SyntaxErrorKind::InvalidFormattedStringExpression))?;
        let expression_slice = &string_body[expr_start..expr_end];

        if let Some(backslash_pos) = expression_slice.find('\\') {
            let abs = token_offset + expr_start + backslash_pos;
            return Err(SyntaxError::new(
                SyntaxErrorKind::BackslashInFStringExpression,
                Span::new(abs, abs + 1),
            ));
        }

        lex_expression_into(expression_slice, token_offset + expr_start, &mut tokens)?;

        cursor = expr_end + 1;
    }

    push_trailing_part(
        &mut tokens,
        &string_body[cursor..],
        Span::new(token_offset + cursor, token_offset + string_body.len()),
        is_first_part,
    );

    for (token, span) in tokens.into_iter().rev() {
        pending_tokens_stack.push((token, span));
    }

    Ok(())
}

struct BodyOffsets {
    start: usize,
    end: usize,
}

/// Locates the body offsets inside the raw `f"..."` / `f'...'` slice, excluding the `f` prefix
/// byte and the surrounding quotes.
fn locate_body(slice: &str, quote_character: char) -> Option<BodyOffsets> {
    let without_prefix = &slice[1..];
    let start = without_prefix.find(quote_character)?;
    let end = without_prefix.rfind(quote_character)?;
    if start == end {
        return None;
    }
    Some(BodyOffsets {
        start: start + 2, // +1 for `f` prefix dropped above, +1 for opening quote.
        end: end + 1,
    })
}

fn find_next_unescaped_brace(body: &str, cursor: usize) -> Option<usize> {
    let mut search_cursor = cursor;
    while let Some(pos) = body[search_cursor..].find('{') {
        let absolute_pos = search_cursor + pos;
        if is_escaped(body, absolute_pos) {
            search_cursor = absolute_pos + 1;
            continue;
        }
        return Some(absolute_pos);
    }
    None
}

fn is_escaped(body: &str, position: usize) -> bool {
    // Byte comparison so a non-ASCII char immediately before the brace does not
    // panic the slice — `&str[a..b]` requires both bounds on UTF-8 boundaries.
    let bytes = body.as_bytes();
    let mut backslash_count = 0;
    let mut i = position;
    while i > 0 && bytes[i - 1] == b'\\' {
        backslash_count += 1;
        i -= 1;
    }
    backslash_count % 2 == 1
}

fn find_matching_close_brace(body: &str, expr_start: usize) -> Option<usize> {
    let mut depth = 1;
    for (i, c) in body[expr_start..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(expr_start + i);
                }
            }
            _ => {}
        }
    }
    None
}

fn push_literal_part(
    tokens: &mut Vec<TokenSpan>,
    literal: &str,
    span: Span,
    is_first_part: &mut bool,
) {
    let token = if *is_first_part {
        Token::FormattedStringStart(Box::new(literal.to_string()))
    } else {
        Token::FormattedStringMiddle(Box::new(literal.to_string()))
    };
    tokens.push((token, span));
    *is_first_part = false;
}

fn push_trailing_part(tokens: &mut Vec<TokenSpan>, literal: &str, span: Span, is_first_part: bool) {
    if is_first_part {
        tokens.push((
            Token::FormattedStringStart(Box::new(literal.to_string())),
            span,
        ));
        // Empty End token marks the boundary so the parser knows the f-string closed.
        tokens.push((
            Token::FormattedStringEnd(Box::default()),
            Span::new(span.end, span.end),
        ));
    } else {
        tokens.push((
            Token::FormattedStringEnd(Box::new(literal.to_string())),
            span,
        ));
    }
}

fn lex_expression_into(
    expression: &str,
    absolute_start: usize,
    tokens: &mut Vec<TokenSpan>,
) -> Result<(), SyntaxError> {
    for token_result in Lexer::new(expression) {
        let (token, span) = token_result?;
        tokens.push((
            token,
            Span::new(absolute_start + span.start, absolute_start + span.end),
        ));
    }
    Ok(())
}

fn error_here(lexer: &LogosLexer<Token>, kind: SyntaxErrorKind) -> SyntaxError {
    SyntaxError::new(kind, Span::new(lexer.span().start, lexer.span().end))
}
