// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::{Span, SyntaxError};
use crate::lexer::Token;

use super::super::Parser;

impl<'source> Parser<'source> {
    pub(crate) fn range_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.additive_expression()?;

        let (range_type, range_token, operator_span) = match &self.lookahead {
            Some((Token::Range, span)) => (RangeExpressionType::Exclusive, Token::Range, *span),
            Some((Token::RangeInclusive, span)) => {
                (RangeExpressionType::Inclusive, Token::RangeInclusive, *span)
            }
            _ => return Ok(start),
        };

        // The bounds carry their own spans, but a diagnostic about the range —
        // mismatched bound types, a non-integer slice — is about the pair, so
        // the range gets a span running from the first bound to the last. A
        // bound built without a span leaves the range starting at the operator,
        // which still lands on the range rather than on the file's first token.
        let span_start = if start.span.is_empty() {
            operator_span.start
        } else {
            start.span.start
        };
        self.eat_token(&range_token)?;
        let end = self.additive_expression()?;
        let span = Span::new(span_start, self.last_consumed_end.max(span_start));
        Ok(ast::range_with_span(
            start,
            Some(Box::new(end)),
            range_type,
            span,
        ))
    }
}
