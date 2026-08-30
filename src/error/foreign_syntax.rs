// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Constructs Miri does not have, recognised where they fail.
//!
//! An author arriving from another language writes its syntax by reflex. Each
//! form here is one the parser or the lexer can name at the point it gives up:
//! the token that failed is unambiguous, and the Miri spelling that replaces it
//! is determined. Recognising the form turns a message about a token into a
//! message about the language.
//!
//! A form carries the byte offsets its repair edits, and nothing else. The
//! offsets are recorded by the check that raised the diagnostic, never
//! recovered by re-reading a message. Whether those bytes still hold what the
//! form named is settled later, when the repair is projected against the source
//! it will edit, so a form never needs the source text to describe itself.

use crate::diagnostics::repair::RepairRequest;

/// A construct from another language, recognised where Miri rejects it.
///
/// A variant carries byte offsets when its Miri counterpart is different text,
/// and nothing when it is a different shape. That split is what decides whether
/// [`ForeignForm::repair`] has anything to offer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForeignForm {
    /// `let x: int` — a type annotation introduced by a colon.
    ColonAnnotation {
        colon_start: usize,
        colon_end: usize,
    },
    /// `fn f() -> int` — a return type introduced by an arrow.
    ArrowReturnType {
        arrow_start: usize,
        arrow_end: usize,
    },
    /// `if x { … }` — a brace-delimited block.
    BraceBlock,
    /// `elif` — a chained branch spelled as one word.
    Elif,
    /// `impl Foo` — methods declared outside the class body.
    ImplBlock,
    /// `println!(…)` — a macro invocation.
    MacroBang { bang_start: usize },
    /// `null`, `nil` or `nullptr` — the absent value under another name.
    NullLiteral {
        spelling_start: usize,
        spelling_end: usize,
    },
    /// `for (k, v) in m` — a for-loop binding a pair.
    TupleForBinding,
    /// `let (a, b) = pair()` — a binding that destructures.
    DestructuringLet,
    /// `let mut x = 5` — a mutable binding spelled `let mut`.
    LetMut {
        keyword_start: usize,
        mut_end: usize,
    },
}

impl ForeignForm {
    /// The Miri form that replaces this one.
    pub fn help(&self) -> &'static str {
        match self {
            Self::ColonAnnotation { .. } => {
                "Miri writes a type annotation without a colon: `let x int = 5`."
            }
            Self::ArrowReturnType { .. } => {
                "Miri writes the return type directly after the parameter list: `fn main() int`."
            }
            Self::BraceBlock => {
                "Miri blocks are indentation-based: end the header with `:` for a \
                 single-line body, or indent the body on the lines below."
            }
            Self::Elif => "Miri spells the chained branch `else if`.",
            Self::ImplBlock => {
                "Miri declares methods inside the class body; there is no `impl` block."
            }
            Self::MacroBang { .. } => "Miri has no macros: call `println(...)` without the `!`.",
            Self::NullLiteral { .. } => "Miri writes the absent value as `None`.",
            Self::TupleForBinding => {
                "Miri's `for` binds a single name: iterate a map with \
                 `for k in m` and read each value with `m.get(k)`."
            }
            Self::DestructuringLet => {
                "Miri has no destructuring binding: bind one name, then read its parts."
            }
            Self::LetMut { .. } => "Miri declares a mutable binding with `var`: `var x = 5`.",
        }
    }

    /// The repair for this form, when its rewrite is textual.
    ///
    /// A form whose Miri counterpart is a different shape rather than different
    /// text — an indented block, a class body, a second binding — has no repair,
    /// because writing it would require inventing code the author has not
    /// written.
    pub fn repair(&self) -> Option<RepairRequest> {
        match self {
            Self::ColonAnnotation {
                colon_start,
                colon_end,
            } => Some(RepairRequest::ColonAnnotation {
                colon_start: *colon_start,
                colon_end: *colon_end,
            }),
            Self::ArrowReturnType {
                arrow_start,
                arrow_end,
            } => Some(RepairRequest::ArrowReturnType {
                arrow_start: *arrow_start,
                arrow_end: *arrow_end,
            }),
            Self::MacroBang { bang_start } => Some(RepairRequest::PrintlnBang {
                bang_start: *bang_start,
            }),
            Self::NullLiteral {
                spelling_start,
                spelling_end,
            } => Some(RepairRequest::NullToNone {
                spelling_start: *spelling_start,
                spelling_end: *spelling_end,
            }),
            Self::LetMut {
                keyword_start,
                mut_end,
            } => Some(RepairRequest::LetMutToVar {
                keyword_start: *keyword_start,
                mut_end: *mut_end,
            }),
            Self::BraceBlock
            | Self::Elif
            | Self::ImplBlock
            | Self::TupleForBinding
            | Self::DestructuringLet => None,
        }
    }
}
