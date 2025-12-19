// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::token::{Token, TokenSpan};
use crate::error::syntax::{SyntaxError, SyntaxErrorKind};
use crate::lexer::Lexer;
use logos::Lexer as LogosLexer;

pub fn lex_formatted_string(
    lexer: &mut LogosLexer<Token>,
    pending_tokens_stack: &mut Vec<TokenSpan>,
    quote_character: char,
) -> Result<(), SyntaxError> {
    let slice = lexer.slice(); // Example: f"Hello, {name}!"
    let without_prefix = &slice[1..]; // remove `f`
    let (start, end) = match (
        without_prefix.find(quote_character),
        without_prefix.rfind(quote_character),
    ) {
        (Some(s), Some(e)) if s != e => (s, e),
        _ => {
            return Err(SyntaxError::new(
                SyntaxErrorKind::InvalidFormattedString,
                lexer.span(),
            ))
        }
    };

    let string_body = &without_prefix[start + 1..end];
    let mut cursor = 0;
    let token_offset = lexer.span().start + 2; // position after f" or f'
    let mut tokens: Vec<TokenSpan> = Vec::new();
    let mut is_first_part = true;

    while cursor < string_body.len() {
        // Manually search for the next unescaped brace from the current cursor.
        let mut next_brace_pos: Option<usize> = None;
        let mut search_cursor = cursor;
        while let Some(pos) = string_body[search_cursor..].find('{') {
            let absolute_pos = search_cursor + pos;

            // Count preceding backslashes to determine if the brace is escaped.
            let mut backslash_count = 0;
            let mut i = absolute_pos;
            while i > 0 && &string_body[i - 1..i] == "\\" {
                backslash_count += 1;
                i -= 1;
            }

            if backslash_count % 2 == 1 {
                // Odd number of backslashes means the brace is escaped.
                // Continue searching after this escaped brace.
                search_cursor = absolute_pos + 1;
                continue;
            } else {
                // Even number of backslashes (or zero) means it's a real expression.
                next_brace_pos = Some(absolute_pos);
                break;
            }
        }

        if let Some(brace_pos) = next_brace_pos {
            // Handle the literal part before the expression
            let literal = &string_body[cursor..brace_pos];
            let literal_span = (token_offset + cursor)..(token_offset + brace_pos);
            if is_first_part {
                tokens.push((
                    Token::FormattedStringStart(literal.to_string()),
                    literal_span,
                ));
            } else {
                tokens.push((
                    Token::FormattedStringMiddle(literal.to_string()),
                    literal_span,
                ));
            }
            is_first_part = false;

            // Find the matching closing brace for the expression
            let mut brace_depth = 1;
            let expr_start = brace_pos + 1;
            let mut expr_end = 0;
            for (i, c) in string_body[expr_start..].char_indices() {
                if c == '{' {
                    brace_depth += 1;
                } else if c == '}' {
                    brace_depth -= 1;
                    if brace_depth == 0 {
                        expr_end = expr_start + i;
                        break;
                    }
                }
            }

            if expr_end == 0 {
                return Err(SyntaxError::new(
                    SyntaxErrorKind::InvalidFormattedStringExpression,
                    lexer.span(),
                ));
            }

            let expression_slice = &string_body[expr_start..expr_end];

            // Check for the disallowed backslash.
            if let Some(backslash_pos) = expression_slice.find('\\') {
                let error_span = (token_offset + expr_start + backslash_pos)
                    ..(token_offset + expr_start + backslash_pos + 1);
                return Err(SyntaxError::new(
                    SyntaxErrorKind::BackslashInFStringExpression,
                    error_span,
                ));
            }

            // Run the sub-lexer on the original, unmodified slice.
            let sub_lexer = Lexer::new(expression_slice);
            for token_result in sub_lexer {
                let (token, span) = token_result?;

                // Calculate the original span with a simple offset.
                let original_start = token_offset + expr_start + span.start;
                let original_end = token_offset + expr_start + span.end;

                tokens.push((token, original_start..original_end));
            }

            cursor = expr_end + 1;
        } else {
            // No more expressions, the rest is the final literal part.
            break;
        }
    }

    // Handle the final literal part after the last expression.
    let final_literal = &string_body[cursor..];
    let final_span = (token_offset + cursor)..(token_offset + string_body.len());
    if is_first_part {
        // The string had no expressions at all.
        tokens.push((
            Token::FormattedStringStart(final_literal.to_string()),
            final_span,
        ));
    } else {
        tokens.push((
            Token::FormattedStringEnd(final_literal.to_string()),
            final_span,
        ));
    }

    // Push all collected tokens onto the pending stack.
    for (token, span) in tokens.into_iter().rev() {
        pending_tokens_stack.push((token, span));
    }

    Ok(())
}
