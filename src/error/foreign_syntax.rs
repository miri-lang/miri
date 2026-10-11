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

use crate::diagnostics::repair::{IfColonKeyword, RepairRequest};

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
    /// `if c then a` — an `if` condition closed by `then` rather than `:`.
    ThenKeyword { then_start: usize },
    /// `if c: a else b` — an inline `else` without the colon its body opens with.
    InlineElseWithoutColon { else_start: usize },
    /// `f"{"a".upper()}"` — a literal inside an interpolation written with the
    /// quote that opened the f-string, which Python 3.12 allows and Miri reads
    /// as the f-string's end. `start..end` is the text between the braces.
    NestedInterpolationQuote {
        start: usize,
        end: usize,
        quote: char,
    },
    /// `!x`, `a && b`, `a || b` — a logical operator spelled with symbols.
    SymbolOperator {
        start: usize,
        operator: SymbolOperator,
    },
}

/// A logical operator other languages spell with symbols and Miri with a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolOperator {
    Not,
    And,
    Or,
}

impl SymbolOperator {
    /// The symbols as the author wrote them.
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Not => "!",
            Self::And => "&&",
            Self::Or => "||",
        }
    }

    /// The word Miri spells the operator with.
    pub fn word(self) -> &'static str {
        match self {
            Self::Not => "not",
            Self::And => "and",
            Self::Or => "or",
        }
    }
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
            Self::ThenKeyword { .. } => {
                "Miri has no `then`: an `if` condition ends with `:`, as in \
                 `if c: a else: b`, or with a new line and an indented block."
            }
            Self::InlineElseWithoutColon { .. } => {
                "An inline `else` takes a colon too: `if c: a else: b`."
            }
            Self::NestedInterpolationQuote { .. } => {
                "A quote inside the braces closes the f-string at that quote, so write \
                 the nested string literal with the other quote character: \
                 `f\"{name.replace('o', '0')}\"`."
            }
            Self::SymbolOperator { operator, .. } => match operator {
                SymbolOperator::Not => "Miri negates with the word `not`: `if not s.is_empty()`.",
                SymbolOperator::And => "Miri spells logical and as the word `and`: `if a and b`.",
                SymbolOperator::Or => "Miri spells logical or as the word `or`: `if a or b`.",
            },
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
            Self::SymbolOperator { start, operator } => Some(RepairRequest::WordOperator {
                start: *start,
                spelling: operator.spelling().to_string(),
                word: operator.word().to_string(),
            }),
            Self::ThenKeyword { then_start } => Some(RepairRequest::IfColon {
                keyword_start: *then_start,
                keyword: IfColonKeyword::Then,
            }),
            Self::InlineElseWithoutColon { else_start } => Some(RepairRequest::IfColon {
                keyword_start: *else_start,
                keyword: IfColonKeyword::Else,
            }),
            Self::NestedInterpolationQuote { start, end, quote } => {
                Some(RepairRequest::InterpolationQuotes {
                    start: *start,
                    end: *end,
                    quote: *quote,
                })
            }
            Self::BraceBlock
            | Self::Elif
            | Self::ImplBlock
            | Self::TupleForBinding
            | Self::DestructuringLet => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_foreign_form_repair_textual_variants() {
        let colon = ForeignForm::ColonAnnotation {
            colon_start: 5,
            colon_end: 6,
        };
        assert_eq!(
            colon.repair(),
            Some(RepairRequest::ColonAnnotation {
                colon_start: 5,
                colon_end: 6,
            })
        );

        let arrow = ForeignForm::ArrowReturnType {
            arrow_start: 10,
            arrow_end: 12,
        };
        assert_eq!(
            arrow.repair(),
            Some(RepairRequest::ArrowReturnType {
                arrow_start: 10,
                arrow_end: 12,
            })
        );

        let macro_bang = ForeignForm::MacroBang { bang_start: 7 };
        assert_eq!(
            macro_bang.repair(),
            Some(RepairRequest::PrintlnBang { bang_start: 7 })
        );

        let null_lit = ForeignForm::NullLiteral {
            spelling_start: 15,
            spelling_end: 19,
        };
        assert_eq!(
            null_lit.repair(),
            Some(RepairRequest::NullToNone {
                spelling_start: 15,
                spelling_end: 19,
            })
        );

        let let_mut = ForeignForm::LetMut {
            keyword_start: 0,
            mut_end: 7,
        };
        assert_eq!(
            let_mut.repair(),
            Some(RepairRequest::LetMutToVar {
                keyword_start: 0,
                mut_end: 7,
            })
        );
    }

    #[test]
    fn test_foreign_form_repair_structural_variants_yield_none() {
        let structural_forms = [
            ForeignForm::BraceBlock,
            ForeignForm::Elif,
            ForeignForm::ImplBlock,
            ForeignForm::TupleForBinding,
            ForeignForm::DestructuringLet,
        ];

        for form in structural_forms {
            assert_eq!(
                form.repair(),
                None,
                "structural foreign form {:?} should not have a textual repair",
                form
            );
        }
    }

    #[test]
    fn test_foreign_form_help_messages() {
        let all_forms = [
            ForeignForm::ColonAnnotation {
                colon_start: 0,
                colon_end: 1,
            },
            ForeignForm::ArrowReturnType {
                arrow_start: 0,
                arrow_end: 2,
            },
            ForeignForm::BraceBlock,
            ForeignForm::Elif,
            ForeignForm::ImplBlock,
            ForeignForm::MacroBang { bang_start: 0 },
            ForeignForm::NullLiteral {
                spelling_start: 0,
                spelling_end: 4,
            },
            ForeignForm::TupleForBinding,
            ForeignForm::DestructuringLet,
            ForeignForm::LetMut {
                keyword_start: 0,
                mut_end: 7,
            },
            ForeignForm::SymbolOperator {
                start: 0,
                operator: SymbolOperator::Not,
            },
            ForeignForm::ThenKeyword { then_start: 0 },
            ForeignForm::InlineElseWithoutColon { else_start: 0 },
        ];

        for form in all_forms {
            let help = form.help();
            assert!(
                !help.is_empty(),
                "help text for {:?} should not be empty",
                form
            );
        }
    }
}
