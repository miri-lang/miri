// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Maps a range of canonical source onto the raw bytes it came from.
//!
//! An anchor is matched against the canonical rendering of a function, where
//! comments and the author's spacing are normalized away, but the replacement
//! has to land in the file the author wrote, leaving everything it did not name
//! untouched. The two texts share a token sequence even though they do not
//! share bytes, so the tokens are what carries a range from one to the other.
//!
//! Both texts are lexed and reduced to their significant tokens; layout tokens
//! are dropped because indentation differs by construction, a method being
//! rendered at the top level. The function's declared name anchors the two
//! streams to each other, and every remaining token is then required to agree
//! in kind and in text. Anything else is a refusal: a construct whose canonical
//! rendering differs from its source, such as redundant parentheses or a
//! literal written `1.50`, cannot be anchored, and guessing at it would edit
//! the wrong bytes.

use crate::error::diagnostic::{Diagnostic, DiagnosticBuilder};
use crate::error::syntax::Span;
use crate::lexer::token::Token;
use crate::lexer::Lexer;

/// Why a canonical rendering could not be anchored to its source.
#[derive(Debug, Clone)]
pub struct AlignmentDiverged {
    /// Position in the significant-token sequence where the two texts parted.
    pub token_index: usize,
    /// What the canonical rendering had there.
    pub expected: String,
    /// What the raw source had there.
    pub actual: String,
    /// Where in the raw source the divergence was seen.
    pub raw_byte_offset: usize,
}

impl AlignmentDiverged {
    /// Describe the divergence to the caller that has to act on it.
    pub fn to_diagnostic(&self) -> Diagnostic {
        use crate::diagnostics::DiagnosticCode;
        DiagnosticBuilder::error(DiagnosticCode::BldSourceNotAnchorable.title().to_string())
            .code(DiagnosticCode::BldSourceNotAnchorable.as_str())
            .message(format!(
                "the source and its canonical form part at token {}: canonical has {}, the file has {} at byte {}",
                self.token_index, self.expected, self.actual, self.raw_byte_offset
            ))
            .help(
                "this function contains something whose canonical form differs from what the file says, such as redundant parentheses around an expression or a literal written 1.50 rather than 1.5; rewrite it in canonical form and the anchor will hold"
                    .to_string(),
            )
            .build()
    }
}

/// One token that carries meaning, as opposed to layout.
#[derive(Debug, Clone)]
struct Significant {
    kind: Token,
    text: String,
    span: Span,
}

impl Significant {
    /// How this token reads in a diagnostic.
    fn describe(&self) -> String {
        format!("{:?} `{}`", self.kind, self.text)
    }
}

/// Layout tokens differ between a nested method and the same method rendered at
/// the top level, so they take no part in the correspondence.
fn is_layout(token: &Token) -> bool {
    matches!(
        token,
        Token::Newline | Token::Indent | Token::Dedent | Token::ExpressionStatementEnd
    )
}

/// Reduce a text to the tokens that carry meaning.
///
/// A text that will not lex yields nothing rather than a prefix: a truncated
/// stream would let a correspondence be built over the part that happened to
/// survive, which is how an edit lands somewhere nobody asked for.
fn significant_tokens(source: &str) -> Option<Vec<Significant>> {
    let lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    for next in lexer {
        let (kind, span) = next.ok()?;
        if is_layout(&kind) {
            continue;
        }
        let text = source.get(span.start..span.end)?.to_string();
        tokens.push(Significant { kind, text, span });
    }
    Some(tokens)
}

/// A verified correspondence between a canonical rendering and its source.
pub struct Alignment {
    canonical: Vec<Significant>,
    raw: Vec<Significant>,
    /// What to add to a canonical token index to reach its raw counterpart.
    offset: usize,
}

impl Alignment {
    /// How many significant tokens the canonical rendering holds.
    pub fn canonical_token_count(&self) -> usize {
        self.canonical.len()
    }

    /// The raw bytes spanned by a run of canonical tokens.
    fn raw_span_of_tokens(&self, first: usize, last: usize) -> Option<(usize, usize)> {
        let start = self.raw.get(first + self.offset)?.span.start;
        let end = self.raw.get(last + self.offset)?.span.end;
        Some((start, end))
    }

    /// The raw bytes a canonical byte range names.
    ///
    /// The range is carried by whole tokens: the first token that starts at or
    /// after the range and the last that ends at or before it. An anchor that
    /// covers no complete token names nothing, and says so rather than
    /// resolving to an empty stretch of the file.
    pub fn raw_range(
        &self,
        canonical_start: usize,
        canonical_end: usize,
    ) -> Option<(usize, usize)> {
        let first = self
            .canonical
            .iter()
            .position(|token| token.span.start >= canonical_start)?;
        let last = self
            .canonical
            .iter()
            .rposition(|token| token.span.end <= canonical_end)?;
        if first > last {
            return None;
        }
        self.raw_span_of_tokens(first, last)
    }

    /// The raw bytes holding everything past a function's header.
    ///
    /// The header's own token count says where the body starts. A body written
    /// after a colon puts that colon between the two, and it belongs to neither
    /// the header nor the body, so it is stepped over.
    pub fn raw_body_range(&self, header_token_count: usize) -> Option<(usize, usize)> {
        let mut first = header_token_count;
        if self
            .canonical
            .get(first)
            .is_some_and(|t| t.kind == Token::Colon)
        {
            first += 1;
        }
        let last = self.canonical.len().checked_sub(1)?;
        if first > last {
            return None;
        }
        self.raw_span_of_tokens(first, last)
    }

    /// The raw bytes this whole declaration occupies.
    pub fn raw_extent(&self) -> Option<(usize, usize)> {
        self.raw_span_of_tokens(0, self.canonical.len().checked_sub(1)?)
    }
}

/// Report a text that would not lex.
fn unlexable(what: &str) -> AlignmentDiverged {
    AlignmentDiverged {
        token_index: 0,
        expected: format!("readable {}", what),
        actual: "text that does not tokenize".to_string(),
        raw_byte_offset: 0,
    }
}

/// Anchor a declaration's canonical rendering to the source it was parsed from.
///
/// The name and name_span are extracted from the declaration being anchored.
pub fn build_alignment(
    raw_source: &str,
    canonical_rendering: &str,
    name: &str,
    name_span: Span,
) -> Result<Alignment, AlignmentDiverged> {
    let canonical =
        significant_tokens(canonical_rendering).ok_or_else(|| unlexable("rendering"))?;
    let raw = significant_tokens(raw_source).ok_or_else(|| unlexable("source"))?;

    // The declared name is the one token both streams can name independently:
    // the parser recorded where it sits in the file, and the renderer emits it
    // after the modifiers and `fn`.
    let raw_name = raw
        .iter()
        .position(|token| token.span.start == name_span.start && token.span.end == name_span.end)
        .ok_or_else(|| AlignmentDiverged {
            token_index: 0,
            expected: format!("the declared name `{}`", name),
            actual: "no token at the recorded name position".to_string(),
            raw_byte_offset: name_span.start,
        })?;
    let canonical_name = canonical
        .iter()
        .position(|token| token.kind == Token::Identifier && token.text == name)
        .ok_or_else(|| AlignmentDiverged {
            token_index: 0,
            expected: format!("the declared name `{}`", name),
            actual: "a rendering that does not name it".to_string(),
            raw_byte_offset: name_span.start,
        })?;

    // A canonical token can only sit at or after its raw counterpart's index
    // when the raw stream also holds everything declared before this function.
    let offset = raw_name
        .checked_sub(canonical_name)
        .ok_or_else(|| AlignmentDiverged {
            token_index: 0,
            expected: "a source holding this whole declaration".to_string(),
            actual: "a source that starts inside it".to_string(),
            raw_byte_offset: name_span.start,
        })?;

    for (index, expected) in canonical.iter().enumerate() {
        let Some(actual) = index.checked_add(offset).and_then(|at| raw.get(at)) else {
            return Err(AlignmentDiverged {
                token_index: index,
                expected: expected.describe(),
                actual: "the end of the file".to_string(),
                raw_byte_offset: raw.last().map_or(0, |token| token.span.end),
            });
        };
        if expected.kind != actual.kind || expected.text != actual.text {
            return Err(AlignmentDiverged {
                token_index: index,
                expected: expected.describe(),
                actual: actual.describe(),
                raw_byte_offset: actual.span.start,
            });
        }
    }

    Ok(Alignment {
        canonical,
        raw,
        offset,
    })
}

/// How many significant tokens a text holds, or nothing if it will not lex.
pub fn significant_token_count(text: &str) -> Option<usize> {
    significant_tokens(text).map(|tokens| tokens.len())
}
