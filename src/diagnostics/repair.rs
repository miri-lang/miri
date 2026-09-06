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
            Self::DropExtraArguments => &[DiagnosticCode::TypTypeMismatch],
            Self::ColonAnnotation => &[DiagnosticCode::ParUnexpectedToken],
            Self::ArrowReturnType => &[DiagnosticCode::ParUnexpectedToken],
            Self::LetMutToVar => &[DiagnosticCode::ParUnexpectedToken],
            Self::NullToNone => &[DiagnosticCode::TypUndefinedName],
            Self::PrintlnBang => &[DiagnosticCode::LexInvalidToken],
            Self::DropIteratorAccessor => &[DiagnosticCode::TypFieldNotFound],
            Self::ConcatToFormattedString => &[DiagnosticCode::TypTypeMismatch],
            Self::QualifyVariantPattern => &[DiagnosticCode::TypEnumVariant],
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
        }
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
