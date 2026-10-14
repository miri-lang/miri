// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Typed repairs attached to diagnostics.
//!
//! A repair is recorded by the check that raised the diagnostic, in terms that
//! check already knows: which bytes name the `let` keyword, which module exports
//! an unresolved name, which byte range holds the surplus arguments. Nothing
//! here re-reads a diagnostic message to recover those facts, so rewording a
//! message can never silently change what a repair edits.
//!
//! This is the inner diagnostics layer, so a request carries plain byte offsets
//! rather than a `Span` — `Span` belongs to the error layer, which depends on
//! this module and never the other way around. Offsets index the source the
//! diagnostic was raised against.

use serde::{Deserialize, Serialize};

use crate::diagnostics::json::{JsonEdit, JsonRepair};
use crate::diagnostics::DiagnosticCode;

/// Stable identifier for a repair shape.
///
/// Wire names are write-once, exactly like diagnostic codes: tooling keys off
/// them, so a name is never renamed and never reused for a different shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RepairId {
    /// Rebind an immutable declaration as mutable.
    LetToVar,
    /// Import a name that resolves in exactly one module.
    AddImport,
    /// Drop positional arguments a call does not declare.
    DropExtraArguments,
    /// Remove a colon and surrounding whitespace in type annotation.
    ColonAnnotation,
    /// Remove an arrow and surrounding whitespace in return type.
    ArrowReturnType,
    /// Replace `let mut` with `var`.
    LetMutToVar,
    /// Replace a null-like literal with `None`.
    NullToNone,
    /// Remove the `!` from a macro call.
    PrintlnBang,
    /// Iterate a keyed collection instead of calling an accessor it does not have.
    DropIteratorAccessor,
    /// Rewrite a chain of `+` joining text and values as a formatted string.
    ConcatToFormattedString,
    /// Prefix a bare variant pattern with the enum that declares it.
    QualifyVariantPattern,
    /// Rewrite a name as the one spelling the help offers in its place.
    RenameToSuggestion,
    /// Prefix a binding nothing reads with an underscore.
    UnderscoreUnreadBinding,
    /// Spell a symbolic logical operator as the word Miri uses.
    WordOperator,
    /// Write the `:` that opens an inline `if` or `else` body.
    IfColon,
    /// Write the string literals inside an interpolation with the other quote.
    InterpolationQuotes,
    /// Delete an import nothing reads.
    RemoveUnusedImport,
    /// Convert a number to the width of the place it is stored with `as`.
    CastToSlotType,
    /// Move statements written beside `main` into it.
    MoveIntoMain,
}

impl RepairId {
    /// The wire string tooling matches on.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LetToVar => "let-to-var",
            Self::AddImport => "add-import",
            Self::DropExtraArguments => "drop-extra-arguments",
            Self::ColonAnnotation => "colon-annotation",
            Self::ArrowReturnType => "arrow-return-type",
            Self::LetMutToVar => "let-mut-to-var",
            Self::NullToNone => "null-to-none",
            Self::PrintlnBang => "println-bang",
            Self::DropIteratorAccessor => "drop-iterator-accessor",
            Self::ConcatToFormattedString => "concat-to-formatted-string",
            Self::QualifyVariantPattern => "qualify-variant-pattern",
            Self::RenameToSuggestion => "rename-to-suggestion",
            Self::UnderscoreUnreadBinding => "underscore-unread-binding",
            Self::WordOperator => "word-operator",
            Self::IfColon => "if-colon",
            Self::InterpolationQuotes => "interpolation-quotes",
            Self::RemoveUnusedImport => "remove-unused-import",
            Self::CastToSlotType => "cast-to-slot-type",
            Self::MoveIntoMain => "move-into-main",
        }
    }

    /// The diagnostic codes a caller can reach this repair from.
    ///
    /// A code appears here only once a test has driven a program that produces
    /// it carrying this repair, so the listing `explain --list` publishes says
    /// what is actually available rather than what was intended.
    pub fn codes(&self) -> &'static [DiagnosticCode] {
        match self {
            Self::LetToVar => &[DiagnosticCode::TypImmutabilityViolation],
            Self::AddImport => &[
                DiagnosticCode::TypUndefinedName,
                DiagnosticCode::TypTypeNotFound,
            ],
            Self::DropExtraArguments => &[DiagnosticCode::TypArgumentCountMismatch],
            Self::ColonAnnotation => &[DiagnosticCode::ParUnexpectedToken],
            Self::ArrowReturnType => &[DiagnosticCode::ParUnexpectedToken],
            Self::LetMutToVar => &[DiagnosticCode::ParUnexpectedToken],
            Self::NullToNone => &[DiagnosticCode::TypUndefinedName],
            Self::PrintlnBang => &[DiagnosticCode::LexInvalidToken],
            Self::DropIteratorAccessor => &[DiagnosticCode::TypFieldNotFound],
            Self::ConcatToFormattedString => &[DiagnosticCode::TypTypeMismatch],
            Self::QualifyVariantPattern => &[DiagnosticCode::TypEnumVariant],
            Self::RenameToSuggestion => &[
                DiagnosticCode::TypFieldNotFound,
                DiagnosticCode::TypEnumVariant,
                DiagnosticCode::TypTypeNotFound,
                DiagnosticCode::TypUndefinedName,
            ],
            Self::UnderscoreUnreadBinding => &[
                DiagnosticCode::TypUnusedLocal,
                DiagnosticCode::TypUnusedParameter,
            ],
            Self::WordOperator => &[
                DiagnosticCode::LexInvalidToken,
                DiagnosticCode::ParUnsupportedCStyleOperator,
            ],
            Self::IfColon => &[DiagnosticCode::ParUnexpectedToken],
            Self::InterpolationQuotes => &[DiagnosticCode::LexInvalidFormattedStringExpression],
            Self::RemoveUnusedImport => &[DiagnosticCode::ImpUnusedImport],
            Self::CastToSlotType => &[DiagnosticCode::TypTypeMismatch],
            Self::MoveIntoMain => &[DiagnosticCode::TypTopLevelStatementBesideMain],
        }
    }

    /// One sentence saying what applying this repair does.
    ///
    /// Published text, not a note to a Rust reader: the skill pack lists these
    /// verbatim and a gate fails when the two disagree, so a repair added here
    /// without a sentence a user can act on will be noticed.
    pub fn summary(&self) -> &'static str {
        match self {
            Self::LetToVar => "Rebind an immutable declaration as mutable.",
            Self::AddImport => "Import a name that resolves in exactly one module.",
            Self::DropExtraArguments => "Drop positional arguments a call does not declare.",
            Self::ColonAnnotation => "Drop the `:` before a type annotation.",
            Self::ArrowReturnType => "Drop the `->` before a return type.",
            Self::LetMutToVar => "Rewrite a `let mut` binding as `var`.",
            Self::NullToNone => "Rewrite `null`, `nil` or `nullptr` as `None`.",
            Self::PrintlnBang => "Drop the `!` from a macro-style call.",
            Self::DropIteratorAccessor => {
                "Drop a `keys` accessor on a keyed collection and iterate it directly."
            }
            Self::ConcatToFormattedString => {
                "Rewrite a `+` chain joining text to values as one f-string."
            }
            Self::QualifyVariantPattern => {
                "Prefix a bare variant pattern with the enum that declares it."
            }
            Self::RenameToSuggestion => {
                "Rewrite a name as the one spelling the help offers in its place."
            }
            Self::UnderscoreUnreadBinding => {
                "Prefix a binding nothing reads with `_` to say the value is not meant to be read."
            }
            Self::WordOperator => "Rewrite `!`, `&&` or `||` as `not`, `and` or `or`.",
            Self::IfColon => "Write `:` in place of `then`, or after an inline `else`.",
            Self::InterpolationQuotes => {
                "Write the string literals inside an f-string's braces with the other quote."
            }
            Self::RemoveUnusedImport => {
                "Delete a `use` line nothing reads, or the unread names in a selection."
            }
            Self::CastToSlotType => {
                "Convert a number with `as` to the width of the place it is stored; a value outside that width's range is truncated or rounded."
            }
            Self::MoveIntoMain => {
                "Move statements written beside `main` into it, in order, ahead of what it already does."
            }
        }
    }

    /// Every repair identifier, in declaration order.
    pub fn all() -> &'static [RepairId] {
        &[
            Self::LetToVar,
            Self::AddImport,
            Self::DropExtraArguments,
            Self::ColonAnnotation,
            Self::ArrowReturnType,
            Self::LetMutToVar,
            Self::NullToNone,
            Self::PrintlnBang,
            Self::DropIteratorAccessor,
            Self::ConcatToFormattedString,
            Self::QualifyVariantPattern,
            Self::RenameToSuggestion,
            Self::UnderscoreUnreadBinding,
            Self::WordOperator,
            Self::IfColon,
            Self::InterpolationQuotes,
            Self::RemoveUnusedImport,
            Self::CastToSlotType,
            Self::MoveIntoMain,
        ]
    }
}

/// The repairs reachable from `code`, in declaration order.
///
/// Availability is a separate question from risk: a code's `fix_safety` is the
/// floor a repair of that condition would have to clear, and it is recorded
/// whether or not a repair exists. This answers the other question — whether
/// one exists at all, and which.
pub fn repairs_for(code: DiagnosticCode) -> Vec<RepairId> {
    RepairId::all()
        .iter()
        .filter(|repair| repair.codes().contains(&code))
        .copied()
        .collect()
}

/// A repair the compiler can perform exactly, recorded where the diagnostic was
/// raised.
///
/// Only conditions whose correct edit is *determined* get a request. A condition
/// whose repair would require inventing a value carries no request at all, so
/// there is no shape here that could write a guess into a source file.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepairRequest {
    /// Rewrite the `let` keyword starting at `keyword_start` as `var`.
    ///
    /// Recorded only for a statement that binds exactly one name. A statement
    /// such as `let a = 1, b = 2` shares one keyword between its bindings, so
    /// rewriting it would make every binding mutable rather than the one the
    /// diagnostic names.
    ///
    /// The `module_scope` flag records whether the binding was declared at module
    /// scope. The `is_public` flag records whether the binding is publicly visible.
    /// Only module-scope public bindings are api-changing; module-scope private
    /// bindings and all function-local bindings are local-edit.
    LetToVar {
        keyword_start: usize,
        module_scope: bool,
        is_public: bool,
    },
    /// Import `name` from `module`.
    ///
    /// Recorded only when exactly one module exports `name`. An ambiguous name
    /// keeps its help text and gets no repair, because picking between the
    /// candidates is the author's decision. Where the `use` line lands is a
    /// question about the text rather than about the program, so the projection
    /// decides it.
    AddImport { module: String, name: String },
    /// Delete the byte range `[start, end)` holding surplus call arguments.
    ///
    /// The range starts after the last argument the callee declares, so the
    /// deletion covers the separating comma and never touches the parentheses.
    DropExtraArguments { start: usize, end: usize },
    /// Remove a colon and surrounding whitespace in a type annotation.
    ColonAnnotation {
        colon_start: usize,
        colon_end: usize,
    },
    /// Remove an arrow and surrounding whitespace in a return type.
    ArrowReturnType {
        arrow_start: usize,
        arrow_end: usize,
    },
    /// Replace `let mut` with `var`.
    LetMutToVar {
        keyword_start: usize,
        mut_end: usize,
    },
    /// Replace a null-like literal with `None`.
    NullToNone {
        spelling_start: usize,
        spelling_end: usize,
    },
    /// Remove the `!` from a macro call.
    PrintlnBang { bang_start: usize },
    /// Delete a member accessor so the receiver is iterated directly.
    ///
    /// `access_start..access_end` covers the whole member access, so the
    /// projection can confirm the recorded accessor really terminates it before
    /// deleting anything. Recorded only for a receiver whose own iteration
    /// already yields what the accessor names, which is decided by the check
    /// that raised the diagnostic.
    DropIteratorAccessor {
        access_start: usize,
        access_end: usize,
        accessor: String,
    },
    /// Rewrite `start..end` — a chain of `+` joining text to values — as one
    /// formatted string.
    ///
    /// `parts` holds the leaves of that chain in source order. A text leaf
    /// contributes its own characters; every other leaf becomes a hole. The
    /// ranges are recorded rather than the text, so the projection reads the
    /// operands out of the source it is given and never out of a message.
    ConcatToFormattedString {
        start: usize,
        end: usize,
        parts: Vec<ConcatPart>,
    },
    /// Prefix each bare variant pattern in `sites` with `enum_name`.
    ///
    /// Every site is one spelling of the same mistake, so they travel on one
    /// request: repairing the first alone would leave the rest to be found on a
    /// second run.
    QualifyVariantPattern {
        enum_name: String,
        sites: Vec<VariantPatternSite>,
    },
    /// Rewrite the `spelling` that ends `start..end` as `replacement`.
    ///
    /// The range is the one the diagnostic underlines, which for a bare name is
    /// the name itself and for a member access is the receiver, the dot and the
    /// member together. Recording the range the reader sees rather than the
    /// name's own offsets means the projection can confirm the name still
    /// terminates it, and a range holding only part of the name — a drifted
    /// offset, or a receiver whose text happens to end in the same letters —
    /// is refused instead of rewriting a receiver.
    RenameToSuggestion {
        start: usize,
        end: usize,
        spelling: String,
        replacement: String,
    },
    /// Insert an underscore before the name at `name_start`.
    ///
    /// The name travels with the offset so the projection can confirm the
    /// binding it was recorded against is still the one at that offset. A name
    /// already spelled with the underscore is never recorded, because the check
    /// that raises the diagnostic treats such a binding as deliberately unread.
    UnderscoreUnreadBinding { name_start: usize, name: String },
    /// Rewrite the symbolic operator `spelling` at `start` as `word`.
    ///
    /// The spelling travels with the offset so the projection can confirm the
    /// symbols are still there before writing the word over them.
    WordOperator {
        start: usize,
        spelling: String,
        word: String,
    },
    /// Write the colon an `if` or `else` body opens with.
    ///
    /// `keyword_start` holds `then`, which the colon replaces, or an inline
    /// `else`, which the colon follows. The keyword travels with the offset so
    /// the projection can confirm it before editing.
    IfColon {
        keyword_start: usize,
        keyword: IfColonKeyword,
    },
    /// Swap every `quote` in `start..end`, the text between an interpolation's
    /// braces, for the other quote character.
    ///
    /// Recorded only for an interpolation holding no other quote and no
    /// backslash, so the swap cannot close or escape anything the author wrote.
    InterpolationQuotes {
        start: usize,
        end: usize,
        quote: char,
    },
    /// Delete the `use` statement at `start..end`, with the line it stands on.
    ///
    /// The projection confirms the range still opens with `use` and that
    /// nothing but whitespace or a comment shares its line, so a drifted range
    /// never deletes code.
    RemoveImportLine { start: usize, end: usize },
    /// Rewrite the names a selective import lists, `start..end` between its
    /// braces, as the `kept` ones alone.
    ///
    /// Every unread name in one selection is reported against the same
    /// request, so applying it once drops them all; the kept names are ranges
    /// rather than text, so an alias travels as written.
    DropSelectedImports {
        start: usize,
        end: usize,
        kept: Vec<(usize, usize)>,
    },
    /// Convert the value at `start..end` to `target` with `as`, inside
    /// parentheses when `parenthesize` says the value is an expression the
    /// cast would otherwise bind to only part of.
    CastToSlotType {
        start: usize,
        end: usize,
        target: String,
        parenthesize: bool,
    },
    /// Move the top-level statements opening at `statements` into `main`,
    /// ahead of the statement `main`'s body opens with at `body_start`.
    ///
    /// Each statement is recorded by where it opens; the projection reads its
    /// extent off the source, as its first line and the more deeply indented
    /// lines under it, because a statement's own span need not cover a block
    /// it carries.
    MoveIntoMain {
        statements: Vec<usize>,
        body_start: usize,
    },
}

/// The word an [`RepairRequest::IfColon`] repair is anchored on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum IfColonKeyword {
    /// `if c then`, where Miri writes `if c:`.
    Then,
    /// `else v` in an inline `if`, where Miri writes `else: v`.
    Else,
}

impl IfColonKeyword {
    fn spelling(self) -> &'static str {
        match self {
            Self::Then => "then",
            Self::Else => "else",
        }
    }
}

/// One leaf of a `+` chain being rewritten as a formatted string.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConcatPart {
    /// First byte of the leaf in the source the diagnostic was raised against.
    pub start: usize,
    /// One past the last byte of the leaf.
    pub end: usize,
    /// Whether the leaf is a text literal, whose contents become literal
    /// characters rather than a hole.
    pub is_text: bool,
}

/// One bare variant pattern awaiting its enum prefix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VariantPatternSite {
    /// First byte of the variant name.
    pub start: usize,
    /// The variant name as written, which the projection confirms is still
    /// there before inserting anything ahead of it.
    pub variant: String,
}

/// The `let` keyword, and the `var` that replaces it. Equal length is a
/// coincidence of the language, not something the edit relies on.
const LET_KEYWORD: &str = "let";

/// The mutability marker other languages spell a mutable binding with. Miri has
/// no such keyword: `var` carries the meaning on its own.
const MUT_KEYWORD: &str = "mut";

impl RepairRequest {
    /// The repair that writes `replacement` where the reader wrote `spelling`,
    /// or nothing when there is no edit to make.
    ///
    /// `start..end` is the range the diagnostic underlines. A range with no
    /// text behind it belongs to a node the compiler synthesized rather than to
    /// something a reader wrote, and a replacement equal to the spelling
    /// describes no change; neither is a repair.
    pub fn rename(start: usize, end: usize, spelling: &str, replacement: &str) -> Option<Self> {
        if start >= end || spelling.is_empty() || spelling == replacement {
            return None;
        }
        Some(Self::RenameToSuggestion {
            start,
            end,
            spelling: spelling.to_string(),
            replacement: replacement.to_string(),
        })
    }

    /// The stable identifier for this request's shape.
    pub fn id(&self) -> RepairId {
        match self {
            Self::LetToVar { .. } => RepairId::LetToVar,
            Self::AddImport { .. } => RepairId::AddImport,
            Self::DropExtraArguments { .. } => RepairId::DropExtraArguments,
            Self::ColonAnnotation { .. } => RepairId::ColonAnnotation,
            Self::ArrowReturnType { .. } => RepairId::ArrowReturnType,
            Self::LetMutToVar { .. } => RepairId::LetMutToVar,
            Self::NullToNone { .. } => RepairId::NullToNone,
            Self::PrintlnBang { .. } => RepairId::PrintlnBang,
            Self::DropIteratorAccessor { .. } => RepairId::DropIteratorAccessor,
            Self::ConcatToFormattedString { .. } => RepairId::ConcatToFormattedString,
            Self::QualifyVariantPattern { .. } => RepairId::QualifyVariantPattern,
            Self::RenameToSuggestion { .. } => RepairId::RenameToSuggestion,
            Self::UnderscoreUnreadBinding { .. } => RepairId::UnderscoreUnreadBinding,
            Self::WordOperator { .. } => RepairId::WordOperator,
            Self::IfColon { .. } => RepairId::IfColon,
            Self::InterpolationQuotes { .. } => RepairId::InterpolationQuotes,
            Self::RemoveImportLine { .. } | Self::DropSelectedImports { .. } => {
                RepairId::RemoveUnusedImport
            }
            Self::CastToSlotType { .. } => RepairId::CastToSlotType,
            Self::MoveIntoMain { .. } => RepairId::MoveIntoMain,
        }
    }

    /// Render this request as concrete edits against `source`.
    ///
    /// Returns `None` when `source` does not hold what the request was recorded
    /// against — a stale offset, or bytes that are not the keyword the request
    /// names. Refusing to emit an edit is the safe outcome: a repair that cannot
    /// be verified is simply not offered.
    pub fn project(&self, path: &str, source: &str) -> Option<JsonRepair> {
        match self {
            Self::LetToVar {
                keyword_start,
                module_scope: _,
                is_public: _,
            } => Self::project_let_to_var(path, source, *keyword_start),
            Self::AddImport { module, name } => {
                Self::project_add_import(path, source, module, name)
            }
            Self::DropExtraArguments { start, end } => {
                Self::project_drop_extra_arguments(path, source, *start, *end)
            }
            Self::ColonAnnotation {
                colon_start,
                colon_end,
            } => Self::project_colon_annotation(path, source, *colon_start, *colon_end),
            Self::ArrowReturnType {
                arrow_start,
                arrow_end,
            } => Self::project_arrow_return_type(path, source, *arrow_start, *arrow_end),
            Self::LetMutToVar {
                keyword_start,
                mut_end,
            } => Self::project_let_mut_to_var(path, source, *keyword_start, *mut_end),
            Self::NullToNone {
                spelling_start,
                spelling_end,
            } => Self::project_null_to_none(path, source, *spelling_start, *spelling_end),
            Self::PrintlnBang { bang_start } => {
                Self::project_println_bang(path, source, *bang_start)
            }
            Self::DropIteratorAccessor {
                access_start,
                access_end,
                accessor,
            } => Self::project_drop_iterator_accessor(
                path,
                source,
                *access_start,
                *access_end,
                accessor,
            ),
            Self::ConcatToFormattedString { start, end, parts } => {
                Self::project_concat_to_formatted_string(path, source, *start, *end, parts)
            }
            Self::QualifyVariantPattern { enum_name, sites } => {
                Self::project_qualify_variant_pattern(path, source, enum_name, sites)
            }
            Self::RenameToSuggestion {
                start,
                end,
                spelling,
                replacement,
            } => Self::project_rename_to_suggestion(
                path,
                source,
                *start,
                *end,
                spelling,
                replacement,
            ),
            Self::UnderscoreUnreadBinding { name_start, name } => {
                Self::project_underscore_unread_binding(path, source, *name_start, name)
            }
            Self::WordOperator {
                start,
                spelling,
                word,
            } => Self::project_word_operator(path, source, *start, spelling, word),
            Self::IfColon {
                keyword_start,
                keyword,
            } => Self::project_if_colon(path, source, *keyword_start, *keyword),
            Self::InterpolationQuotes { start, end, quote } => {
                Self::project_interpolation_quotes(path, source, *start, *end, *quote)
            }
            Self::RemoveImportLine { start, end } => {
                Self::project_remove_import_line(path, source, *start, *end)
            }
            Self::DropSelectedImports { start, end, kept } => {
                Self::project_drop_selected_imports(path, source, *start, *end, kept)
            }
            Self::CastToSlotType {
                start,
                end,
                target,
                parenthesize,
            } => {
                Self::project_cast_to_slot_type(path, source, (*start, *end), target, *parenthesize)
            }
            Self::MoveIntoMain {
                statements,
                body_start,
            } => Self::project_move_into_main(path, source, statements, *body_start),
        }
    }

    /// Delete each statement's lines and write them, indented as `main`'s
    /// body is, at the start of that body.
    fn project_move_into_main(
        path: &str,
        source: &str,
        statements: &[usize],
        body_start: usize,
    ) -> Option<JsonRepair> {
        let body_line = line_start(source, body_start)?;
        let indent_len = source[body_line..]
            .find(|c: char| c != ' ' && c != '\t')
            .unwrap_or(0);
        let indent = &source[body_line..body_line + indent_len];
        if statements.is_empty() || indent.is_empty() {
            return None;
        }
        let mut moved = String::new();
        let mut edits = Vec::new();
        for &start in statements {
            let (from, to) = top_level_extent(source, start)?;
            if (from..to).contains(&body_line) {
                return None;
            }
            for line in source[from..to].split_inclusive('\n') {
                if !line.trim().is_empty() {
                    moved.push_str(indent);
                }
                moved.push_str(line);
            }
            if !moved.ends_with('\n') {
                moved.push('\n');
            }
            edits.push(JsonEdit {
                path: path.to_string(),
                start: from,
                end: to,
                replacement: String::new(),
            });
        }
        edits.push(JsonEdit {
            path: path.to_string(),
            start: body_line,
            end: body_line,
            replacement: moved,
        });
        Some(JsonRepair {
            id: RepairId::MoveIntoMain.as_str().to_string(),
            summary: "Move the statements beside `main` to the start of its body.".to_string(),
            edits,
        })
    }

    /// Append `as target` to the value, wrapping it in parentheses first
    /// when asked.
    fn project_cast_to_slot_type(
        path: &str,
        source: &str,
        (start, end): (usize, usize),
        target: &str,
        parenthesize: bool,
    ) -> Option<JsonRepair> {
        if start >= end || source.get(start..end)?.trim().is_empty() {
            return None;
        }
        let insert = |at: usize, text: String| JsonEdit {
            path: path.to_string(),
            start: at,
            end: at,
            replacement: text,
        };
        let edits = if parenthesize {
            vec![
                insert(start, "(".to_string()),
                insert(end, format!(") as {target}")),
            ]
        } else {
            vec![insert(end, format!(" as {target}"))]
        };
        Some(JsonRepair {
            id: RepairId::CastToSlotType.as_str().to_string(),
            summary: format!(
                "Convert the value with `as {target}`; a value outside its range is truncated or rounded."
            ),
            edits,
        })
    }

    /// Delete the whole line the `use` at `start..end` stands on, newline
    /// included, when the statement is the only code on it.
    fn project_remove_import_line(
        path: &str,
        source: &str,
        start: usize,
        end: usize,
    ) -> Option<JsonRepair> {
        let statement = source.get(start..end)?.trim_end();
        if !statement.starts_with("use ") {
            return None;
        }
        let line_start = source.get(..start)?.rfind('\n').map_or(0, |at| at + 1);
        let statement_end = start + statement.len();
        let line_end = source[statement_end..]
            .find('\n')
            .map_or(source.len(), |at| statement_end + at + 1);
        let before = &source[line_start..start];
        let after = source[statement_end..line_end].trim();
        if !before.trim().is_empty() || !(after.is_empty() || after.starts_with("//")) {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::RemoveUnusedImport.as_str().to_string(),
            summary: "Delete the import nothing reads.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: line_start,
                end: line_end,
                replacement: String::new(),
            }],
        })
    }

    /// Rewrite a selection's names as the kept ones, joined the way the
    /// formatter writes them.
    fn project_drop_selected_imports(
        path: &str,
        source: &str,
        start: usize,
        end: usize,
        kept: &[(usize, usize)],
    ) -> Option<JsonRepair> {
        if kept.is_empty() || !source.get(..start)?.trim_end().ends_with('{') {
            return None;
        }
        let names = kept
            .iter()
            .map(|&(from, to)| {
                (start <= from && to <= end)
                    .then(|| source.get(from..to))
                    .flatten()
                    .filter(|name| !name.trim().is_empty())
            })
            .collect::<Option<Vec<_>>>()?;
        source.get(start..end)?;
        Some(JsonRepair {
            id: RepairId::RemoveUnusedImport.as_str().to_string(),
            summary: "Drop the names nothing reads from the selection.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement: names.join(", "),
            }],
        })
    }

    fn project_let_to_var(path: &str, source: &str, keyword_start: usize) -> Option<JsonRepair> {
        let end = keyword_start.checked_add(LET_KEYWORD.len())?;
        // Confirm the bytes about to be replaced really are the keyword. This
        // is what keeps a drifting span from rewriting an unrelated token.
        if source.get(keyword_start..end)? != LET_KEYWORD {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::LetToVar.as_str().to_string(),
            summary: "Declare the variable with `var` so it can be reassigned.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: keyword_start,
                end,
                replacement: "var".to_string(),
            }],
        })
    }

    fn project_add_import(
        path: &str,
        source: &str,
        module: &str,
        name: &str,
    ) -> Option<JsonRepair> {
        let insert_at = import_insertion_offset(source);
        Some(JsonRepair {
            id: RepairId::AddImport.as_str().to_string(),
            summary: format!("Import `{}` from `{}`.", name, module),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: insert_at,
                end: insert_at,
                replacement: format!("use {}.{{{}}}\n", module, name),
            }],
        })
    }

    fn project_drop_extra_arguments(
        path: &str,
        source: &str,
        start: usize,
        end: usize,
    ) -> Option<JsonRepair> {
        if start >= end || end > source.len() {
            return None;
        }
        if !source.is_char_boundary(start) || !source.is_char_boundary(end) {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::DropExtraArguments.as_str().to_string(),
            summary: "Remove the arguments the function does not declare.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement: String::new(),
            }],
        })
    }

    fn project_colon_annotation(
        path: &str,
        source: &str,
        colon_start: usize,
        colon_end: usize,
    ) -> Option<JsonRepair> {
        let separator = replace_with_separator(source, colon_start, colon_end, ":")?;
        Some(JsonRepair {
            id: RepairId::ColonAnnotation.as_str().to_string(),
            summary: "Write the type after the name, without a colon.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: separator.start,
                end: separator.end,
                replacement: " ".to_string(),
            }],
        })
    }

    fn project_arrow_return_type(
        path: &str,
        source: &str,
        arrow_start: usize,
        arrow_end: usize,
    ) -> Option<JsonRepair> {
        let separator = replace_with_separator(source, arrow_start, arrow_end, "->")?;
        Some(JsonRepair {
            id: RepairId::ArrowReturnType.as_str().to_string(),
            summary: "Write the return type after the parameter list, without an arrow."
                .to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: separator.start,
                end: separator.end,
                replacement: " ".to_string(),
            }],
        })
    }

    fn project_let_mut_to_var(
        path: &str,
        source: &str,
        keyword_start: usize,
        mut_end: usize,
    ) -> Option<JsonRepair> {
        // This edit replaces a whole span rather than one token, so confirming
        // its two ends is not enough: whatever sits between them is deleted as
        // well. Requiring the gap to be blank is what keeps the edit from
        // swallowing a comment written between the two words.
        let keyword_end = keyword_start.checked_add(LET_KEYWORD.len())?;
        if source.get(keyword_start..keyword_end)? != LET_KEYWORD {
            return None;
        }
        let mut_start = mut_end.checked_sub(MUT_KEYWORD.len())?;
        if source.get(mut_start..mut_end)? != MUT_KEYWORD {
            return None;
        }
        if !source.get(keyword_end..mut_start)?.trim().is_empty() {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::LetMutToVar.as_str().to_string(),
            summary: "Replace `let mut` with `var`.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: keyword_start,
                end: mut_end,
                replacement: "var".to_string(),
            }],
        })
    }

    fn project_null_to_none(
        path: &str,
        source: &str,
        spelling_start: usize,
        spelling_end: usize,
    ) -> Option<JsonRepair> {
        let spelling = source.get(spelling_start..spelling_end)?;
        if !matches!(spelling, "null" | "nil" | "nullptr") {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::NullToNone.as_str().to_string(),
            summary: "Replace the null literal with `None`.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: spelling_start,
                end: spelling_end,
                replacement: "None".to_string(),
            }],
        })
    }

    fn project_drop_iterator_accessor(
        path: &str,
        source: &str,
        access_start: usize,
        access_end: usize,
        accessor: &str,
    ) -> Option<JsonRepair> {
        let access = source.get(access_start..access_end)?;
        let suffix = format!(".{}", accessor);
        // The accessor must still terminate the member access. A drifted offset
        // that lands mid-expression would otherwise delete a receiver.
        if !access.ends_with(&suffix) {
            return None;
        }
        let start = access_end.checked_sub(suffix.len())?;
        let end = empty_call_end(source, access_end)?;
        Some(JsonRepair {
            id: RepairId::DropIteratorAccessor.as_str().to_string(),
            summary: format!(
                "Iterate the value itself instead of calling `{}`.",
                accessor
            ),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement: String::new(),
            }],
        })
    }

    fn project_concat_to_formatted_string(
        path: &str,
        source: &str,
        start: usize,
        end: usize,
        parts: &[ConcatPart],
    ) -> Option<JsonRepair> {
        if parts.is_empty() || start >= end || end > source.len() {
            return None;
        }
        let mut rendered = String::from("f\"");
        let mut reach = start;
        for part in parts {
            // Parts tile the expression left to right. A part that overlaps the
            // one before it, or that reaches outside the expression, is not a
            // reading of this source, so nothing is written.
            if part.start < reach || part.end > end || part.start >= part.end {
                return None;
            }
            let text = source.get(part.start..part.end)?;
            if part.is_text {
                rendered.push_str(literal_body(text)?);
            } else {
                rendered.push('{');
                rendered.push_str(hole_body(text)?);
                rendered.push('}');
            }
            reach = part.end;
        }
        rendered.push('"');
        Some(JsonRepair {
            id: RepairId::ConcatToFormattedString.as_str().to_string(),
            summary: "Build the text with a formatted string.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement: rendered,
            }],
        })
    }

    fn project_qualify_variant_pattern(
        path: &str,
        source: &str,
        enum_name: &str,
        sites: &[VariantPatternSite],
    ) -> Option<JsonRepair> {
        if sites.is_empty() {
            return None;
        }
        let mut edits = Vec::with_capacity(sites.len());
        let mut reach = 0;
        for site in sites {
            // Ascending, non-overlapping offsets keep the insertions
            // independent of one another and of the order they are applied in.
            if site.start < reach {
                return None;
            }
            let end = site.start.checked_add(site.variant.len())?;
            if source.get(site.start..end)? != site.variant {
                return None;
            }
            edits.push(JsonEdit {
                path: path.to_string(),
                start: site.start,
                end: site.start,
                replacement: format!("{}.", enum_name),
            });
            reach = end;
        }
        Some(JsonRepair {
            id: RepairId::QualifyVariantPattern.as_str().to_string(),
            summary: format!("Name the enum the variant belongs to: `{}.`.", enum_name),
            edits,
        })
    }

    fn project_rename_to_suggestion(
        path: &str,
        source: &str,
        start: usize,
        end: usize,
        spelling: &str,
        replacement: &str,
    ) -> Option<JsonRepair> {
        // A replacement equal to what is already written is not an edit. The
        // checks that record this never offer one, and a request that somehow
        // carries one describes no change, which a repair cannot represent.
        if spelling.is_empty() || spelling == replacement {
            return None;
        }
        let underlined = source.get(start..end)?;
        if !underlined.ends_with(spelling) {
            return None;
        }
        let name_start = end.checked_sub(spelling.len())?;
        // Anything ahead of the name inside the underlined range is a receiver,
        // so a dot must separate the two. Without one the range ends in the
        // same letters by accident and the name is not what is written there.
        if name_start > start && !underlined[..name_start - start].ends_with('.') {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::RenameToSuggestion.as_str().to_string(),
            summary: format!("Write `{}` instead of `{}`.", replacement, spelling),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: name_start,
                end,
                replacement: replacement.to_string(),
            }],
        })
    }

    fn project_underscore_unread_binding(
        path: &str,
        source: &str,
        name_start: usize,
        name: &str,
    ) -> Option<JsonRepair> {
        let end = name_start.checked_add(name.len())?;
        if name.is_empty() || source.get(name_start..end)? != name {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::UnderscoreUnreadBinding.as_str().to_string(),
            summary: format!("Name the binding `_{}` to say nothing reads it.", name),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: name_start,
                end: name_start,
                replacement: "_".to_string(),
            }],
        })
    }

    /// Write `word` over the symbols, with a space on each side the symbols'
    /// neighbours do not already supply: `a&&b` and `!x` need them, because a
    /// word run into its operand reads as one longer name.
    fn project_word_operator(
        path: &str,
        source: &str,
        start: usize,
        spelling: &str,
        word: &str,
    ) -> Option<JsonRepair> {
        let end = start.checked_add(spelling.len())?;
        if spelling.is_empty() || source.get(start..end)? != spelling {
            return None;
        }
        let spaced = |text: Option<char>| text.is_some_and(|c| !c.is_whitespace() && c != '(');
        let before = source.get(..start)?.chars().next_back();
        let after = source.get(end..)?.chars().next();
        let mut replacement = String::new();
        if spaced(before) {
            replacement.push(' ');
        }
        replacement.push_str(word);
        if after.is_some_and(|c| !c.is_whitespace()) {
            replacement.push(' ');
        }
        Some(JsonRepair {
            id: RepairId::WordOperator.as_str().to_string(),
            summary: format!("Write `{}` as `{}`.", spelling, word),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement,
            }],
        })
    }

    /// `then` gives way to a colon pressed against the condition; an inline
    /// `else` keeps its place and gains the colon after it.
    fn project_if_colon(
        path: &str,
        source: &str,
        keyword_start: usize,
        keyword: IfColonKeyword,
    ) -> Option<JsonRepair> {
        let spelling = keyword.spelling();
        let keyword_end = keyword_start.checked_add(spelling.len())?;
        if source.get(keyword_start..keyword_end)? != spelling {
            return None;
        }
        let (start, end, replacement) = match keyword {
            IfColonKeyword::Then => {
                let condition_end = source.get(..keyword_start)?.trim_end().len();
                (condition_end, keyword_end, ":")
            }
            IfColonKeyword::Else => (keyword_end, keyword_end, ":"),
        };
        Some(JsonRepair {
            id: RepairId::IfColon.as_str().to_string(),
            summary: match keyword {
                IfColonKeyword::Then => "Write `:` in place of `then`.".to_string(),
                IfColonKeyword::Else => "Write `:` after the inline `else`.".to_string(),
            },
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement: replacement.to_string(),
            }],
        })
    }

    fn project_interpolation_quotes(
        path: &str,
        source: &str,
        start: usize,
        end: usize,
        quote: char,
    ) -> Option<JsonRepair> {
        let other = other_quote(quote)?;
        let text = source.get(start..end)?;
        if !text.contains(quote) || text.contains(other) || text.contains(['\\', '\n']) {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::InterpolationQuotes.as_str().to_string(),
            summary: format!("Write the literals inside the braces with {}.", other),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start,
                end,
                replacement: text.replace(quote, &other.to_string()),
            }],
        })
    }

    fn project_println_bang(path: &str, source: &str, bang_start: usize) -> Option<JsonRepair> {
        if source.get(bang_start..bang_start + 1)? != "!" {
            return None;
        }
        Some(JsonRepair {
            id: RepairId::PrintlnBang.as_str().to_string(),
            summary: "Remove the macro invocation operator.".to_string(),
            edits: vec![JsonEdit {
                path: path.to_string(),
                start: bang_start,
                end: bang_start + 1,
                replacement: String::new(),
            }],
        })
    }
}

/// The quote character that does not close a string opened with `quote`.
pub fn other_quote(quote: char) -> Option<char> {
    match quote {
        '"' => Some('\''),
        '\'' => Some('"'),
        _ => None,
    }
}

/// Characters a formatted string cannot carry verbatim.
///
/// A brace would open or close a hole, a quote would close the string, and a
/// backslash would start an escape whose meaning belongs to the original
/// spelling rather than to the rewrite. Any of them means the rewrite is not
/// determined, and an undetermined repair is not offered.
const FORMATTED_STRING_HAZARDS: [char; 4] = ['{', '}', '"', '\\'];

/// The characters between a text literal's quotes, or `None` when they cannot
/// stand inside a formatted string unchanged.
fn literal_body(text: &str) -> Option<&str> {
    let body = text.strip_prefix('"')?.strip_suffix('"')?;
    if body.contains(FORMATTED_STRING_HAZARDS) || body.contains('\n') {
        return None;
    }
    Some(body)
}

/// The source of an operand that is to become a hole, or `None` when it cannot
/// stand inside one.
fn hole_body(text: &str) -> Option<&str> {
    if text.contains(FORMATTED_STRING_HAZARDS) || text.contains('\n') {
        return None;
    }
    Some(text)
}

/// One past the end of an empty argument list starting at `offset`, `offset`
/// itself when no call follows, or `None` when the call carries arguments.
///
/// Deleting an accessor has to take the parentheses that called it with it. A
/// call that passes something is not the shape this repair was recorded for, so
/// it yields no edit rather than an edit that drops an argument.
fn empty_call_end(source: &str, offset: usize) -> Option<usize> {
    let rest = source.get(offset..)?;
    let before_open = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let open = rest.get(before_open..)?;
    let Some(inside) = open.strip_prefix('(') else {
        return Some(offset);
    };
    let closing = inside.trim_start_matches([' ', '\t']);
    if !closing.starts_with(')') {
        return None;
    }
    Some(offset + before_open + 1 + (inside.len() - closing.len()) + 1)
}

/// A byte range to be replaced, and what surrounds it.
struct SeparatorRange {
    start: usize,
    end: usize,
}

/// The range a punctuation token occupies together with the whitespace hugging it.
///
/// Miri separates the two sides of these constructs with a space where the
/// foreign spelling puts a token. Deleting only the token would run the sides
/// together when nothing spaced them (`let x:int`), and leave a double space
/// when something did (`let x: int`). Absorbing the surrounding whitespace and
/// writing back exactly one space is correct in both, and the verification that
/// `token` really sits at the recorded offsets is what makes it safe to do.
fn replace_with_separator(
    source: &str,
    start: usize,
    end: usize,
    token: &str,
) -> Option<SeparatorRange> {
    if source.get(start..end)? != token {
        return None;
    }

    let leading = source.get(..start)?;
    let absorbed_start = leading.trim_end_matches([' ', '\t']).len();

    let trailing = source.get(end..)?;
    let absorbed_end = end + (trailing.len() - trailing.trim_start_matches([' ', '\t']).len());

    Some(SeparatorRange {
        start: absorbed_start,
        end: absorbed_end,
    })
}

/// The byte offset at which a new `use` line belongs.
///
/// A file that already imports gets the new line directly after its last
/// top-level `use`, keeping imports together. A file with none gets it after
/// any leading comment header, so the licence block stays at the top.
fn import_insertion_offset(source: &str) -> usize {
    let mut offset = 0;
    let mut after_last_use = None;
    let mut after_header = None;

    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let line_end = offset + line.len();
        if trimmed.starts_with("use ") {
            after_last_use = Some(line_end);
        } else if after_last_use.is_none()
            && after_header.is_none()
            && !trimmed.is_empty()
            && !trimmed.starts_with("//")
        {
            after_header = Some(offset);
        }
        offset = line_end;
    }

    after_last_use.or(after_header).unwrap_or(0)
}

/// Where the line holding `offset` begins.
fn line_start(source: &str, offset: usize) -> Option<usize> {
    Some(source.get(..offset)?.rfind('\n').map_or(0, |at| at + 1))
}

/// The lines of the top-level statement opening at `start`: its own line,
/// every more deeply indented line under it, and the lines at its own
/// indentation that continue it — an `else` after an `if`, the `while` that
/// closes a `do` — through the newline of the last one, leaving trailing blank
/// lines where they are. `None` unless `start` opens a line, as a top-level
/// statement does.
fn top_level_extent(source: &str, start: usize) -> Option<(usize, usize)> {
    if line_start(source, start)? != start {
        return None;
    }
    let mut awaiting_do_condition = opens_with_word(&source[start..], "do");
    let mut end = next_line(source, start);
    let mut cursor = end;
    while cursor < source.len() {
        let line_end = next_line(source, cursor);
        let line = &source[cursor..line_end];
        let continues = if line.trim().is_empty() {
            false
        } else if line.starts_with([' ', '\t']) || opens_with_word(line, "else") {
            true
        } else if awaiting_do_condition && opens_with_word(line, "while") {
            awaiting_do_condition = false;
            true
        } else {
            break;
        };
        if continues {
            end = line_end;
        }
        cursor = line_end;
    }
    Some((start, end))
}

/// Where the line after the one holding `offset` begins, or the end of
/// `source`.
fn next_line(source: &str, offset: usize) -> usize {
    source[offset..]
        .find('\n')
        .map_or(source.len(), |at| offset + at + 1)
}

/// Whether `line` opens with the keyword `word`, as a whole word.
fn opens_with_word(line: &str, word: &str) -> bool {
    line.strip_prefix(word)
        .is_some_and(|rest| rest.starts_with([' ', '\t', '\n', '\r', ':']) || rest.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewriting_a_keyword_requires_that_keyword_to_be_there() {
        let request = RepairRequest::LetToVar {
            keyword_start: 0,
            module_scope: false,
            is_public: false,
        };

        assert!(request.project("main.mi", "let a = 1\n").is_some());
        assert!(
            request.project("main.mi", "var a = 1\n").is_none(),
            "an offset that does not name `let` must yield no edit"
        );
        assert!(
            request.project("main.mi", "").is_none(),
            "an offset past the end of the source must yield no edit"
        );
    }

    #[test]
    fn test_an_import_leads_a_file_that_has_none() {
        let request = RepairRequest::AddImport {
            module: "system.math".to_string(),
            name: "sqrt".to_string(),
        };

        let repair = request
            .project("main.mi", "fn main()\n    sqrt(4.0)\n")
            .expect("an import is always placeable");

        assert_eq!(repair.edits[0].start, 0);
        assert_eq!(repair.edits[0].replacement, "use system.math.{sqrt}\n");
    }

    #[test]
    fn test_an_import_follows_a_licence_header_rather_than_preceding_it() {
        let source = "// SPDX-License-Identifier: Apache-2.0\n\nfn main()\n    sqrt(4.0)\n";
        let request = RepairRequest::AddImport {
            module: "system.math".to_string(),
            name: "sqrt".to_string(),
        };

        let repair = request
            .project("main.mi", source)
            .expect("an import is always placeable");

        let offset = repair.edits[0].start;
        assert!(
            source[..offset].starts_with("// SPDX"),
            "the header should stay above the import"
        );
        assert!(source[offset..].starts_with("fn main()"));
    }

    #[test]
    fn test_an_import_joins_the_imports_already_present() {
        let source = "use system.io.{println}\n\nfn main()\n    sqrt(4.0)\n";
        let request = RepairRequest::AddImport {
            module: "system.math".to_string(),
            name: "sqrt".to_string(),
        };

        let repair = request
            .project("main.mi", source)
            .expect("an import is always placeable");

        assert_eq!(repair.edits[0].start, "use system.io.{println}\n".len());
    }

    #[test]
    fn test_a_deletion_outside_the_source_yields_no_edit() {
        let request = RepairRequest::DropExtraArguments { start: 2, end: 99 };

        assert!(request.project("main.mi", "add(1)").is_none());
    }

    #[test]
    fn test_dropping_an_accessor_takes_its_parentheses_with_it() {
        let source = "for k in tags.keys()\n";
        let request = RepairRequest::DropIteratorAccessor {
            access_start: 9,
            access_end: 18,
            accessor: "keys".to_string(),
        };

        let repair = request
            .project("main.mi", source)
            .expect("the accessor is where it was recorded");
        let edit = &repair.edits[0];

        assert_eq!(&source[edit.start..edit.end], ".keys()");
        assert!(edit.replacement.is_empty());
    }

    #[test]
    fn test_an_accessor_called_with_an_argument_is_not_dropped() {
        // Deleting the accessor would delete the argument with it, which is a
        // different program rather than the same one spelled directly.
        let request = RepairRequest::DropIteratorAccessor {
            access_start: 0,
            access_end: 9,
            accessor: "keys".to_string(),
        };

        assert!(request.project("main.mi", "tags.keys(1)\n").is_none());
    }

    #[test]
    fn test_a_rename_rewrites_the_member_and_leaves_the_receiver() {
        let source = "println(accounts.len())\n";
        let request = RepairRequest::rename(8, 20, "len", "length")
            .expect("a rename with a different spelling is an edit");

        let repair = request
            .project("main.mi", source)
            .expect("the member is where it was recorded");
        let edit = &repair.edits[0];

        assert_eq!(&source[edit.start..edit.end], "len");
        assert_eq!(edit.replacement, "length");
    }

    #[test]
    fn test_a_rename_of_a_bare_name_covers_the_whole_range() {
        let source = "let x Widgt = 1\n";
        let request = RepairRequest::rename(6, 11, "Widgt", "Widget")
            .expect("a rename with a different spelling is an edit");

        let repair = request
            .project("main.mi", source)
            .expect("the name is where it was recorded");
        let edit = &repair.edits[0];

        assert_eq!(&source[edit.start..edit.end], "Widgt");
        assert_eq!(edit.replacement, "Widget");
    }

    #[test]
    fn test_a_rename_to_the_spelling_already_written_is_not_an_edit() {
        assert!(RepairRequest::rename(0, 3, "len", "len").is_none());
    }

    #[test]
    fn test_a_rename_whose_range_ends_mid_receiver_is_refused() {
        // `xlen` ends in the recorded spelling without being it. Renaming there
        // would rewrite the tail of the receiver rather than the member.
        let request = RepairRequest::rename(0, 4, "len", "length")
            .expect("a rename with a different spelling is an edit");

        assert!(request.project("main.mi", "xlen.count()\n").is_none());
    }

    #[test]
    fn test_a_rename_against_source_that_moved_is_refused() {
        let request = RepairRequest::rename(8, 20, "len", "length")
            .expect("a rename with a different spelling is an edit");

        assert!(request.project("main.mi", "println(x)\n").is_none());
    }

    #[test]
    fn test_an_underscore_is_inserted_ahead_of_the_name_alone() {
        let source = "    let total = 42\n";
        let request = RepairRequest::UnderscoreUnreadBinding {
            name_start: 8,
            name: "total".to_string(),
        };

        let repair = request
            .project("main.mi", source)
            .expect("the name is where it was recorded");
        let edit = &repair.edits[0];

        assert_eq!(edit.start, 8);
        assert_eq!(edit.end, 8, "the insertion replaces nothing");
        assert_eq!(edit.replacement, "_");
    }

    #[test]
    fn test_an_underscore_is_not_inserted_where_the_name_is_no_longer_written() {
        let request = RepairRequest::UnderscoreUnreadBinding {
            name_start: 8,
            name: "total".to_string(),
        };

        assert!(request.project("main.mi", "    let sum = 42\n").is_none());
    }

    #[test]
    fn test_an_accessor_that_moved_is_not_dropped() {
        let request = RepairRequest::DropIteratorAccessor {
            access_start: 0,
            access_end: 11,
            accessor: "keys".to_string(),
        };

        assert!(request.project("main.mi", "tags.values()\n").is_none());
    }

    #[test]
    fn test_a_join_becomes_one_formatted_string() {
        let source = "let s = \"x\" + \" \" + qty\n";
        let request = RepairRequest::ConcatToFormattedString {
            start: 8,
            end: 23,
            parts: vec![
                ConcatPart {
                    start: 8,
                    end: 11,
                    is_text: true,
                },
                ConcatPart {
                    start: 14,
                    end: 17,
                    is_text: true,
                },
                ConcatPart {
                    start: 20,
                    end: 23,
                    is_text: false,
                },
            ],
        };

        let repair = request
            .project("main.mi", source)
            .expect("every part is inside the expression");

        assert_eq!(repair.edits[0].replacement, "f\"x {qty}\"");
    }

    #[test]
    fn test_a_join_carrying_a_brace_is_not_rewritten() {
        // A brace in the text would open a hole in the string it lands in, so
        // the rewrite is not the same text and is not offered.
        let source = "let s = \"{\" + qty\n";
        let request = RepairRequest::ConcatToFormattedString {
            start: 8,
            end: 18,
            parts: vec![
                ConcatPart {
                    start: 8,
                    end: 11,
                    is_text: true,
                },
                ConcatPart {
                    start: 14,
                    end: 17,
                    is_text: false,
                },
            ],
        };

        assert!(request.project("main.mi", source).is_none());
    }

    #[test]
    fn test_parts_that_overlap_yield_no_rewrite() {
        let source = "let s = \"x\" + qty\n";
        let request = RepairRequest::ConcatToFormattedString {
            start: 8,
            end: 18,
            parts: vec![
                ConcatPart {
                    start: 8,
                    end: 12,
                    is_text: true,
                },
                ConcatPart {
                    start: 10,
                    end: 14,
                    is_text: false,
                },
            ],
        };

        assert!(request.project("main.mi", source).is_none());
    }

    #[test]
    fn test_every_bare_variant_is_prefixed_once() {
        let source = "        Ok(n)\n        Err(e)\n";
        let request = RepairRequest::QualifyVariantPattern {
            enum_name: "Result".to_string(),
            sites: vec![
                VariantPatternSite {
                    start: 8,
                    variant: "Ok".to_string(),
                },
                VariantPatternSite {
                    start: 22,
                    variant: "Err".to_string(),
                },
            ],
        };

        let repair = request
            .project("main.mi", source)
            .expect("both variants are where they were recorded");

        assert_eq!(repair.edits.len(), 2);
        for edit in &repair.edits {
            assert_eq!(
                edit.start, edit.end,
                "a prefix inserts rather than replaces"
            );
            assert_eq!(edit.replacement, "Result.");
        }
    }

    #[test]
    fn test_a_variant_that_moved_is_not_prefixed() {
        let request = RepairRequest::QualifyVariantPattern {
            enum_name: "Result".to_string(),
            sites: vec![VariantPatternSite {
                start: 8,
                variant: "Ok".to_string(),
            }],
        };

        assert!(request.project("main.mi", "        None\n").is_none());
    }

    #[test]
    fn test_every_repair_identifier_has_a_distinct_wire_name() {
        let mut names: Vec<&str> = RepairId::all().iter().map(RepairId::as_str).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), total, "repair identifiers must be unique");
    }
}
