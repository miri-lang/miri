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
        ]
    }
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
    fn test_every_repair_identifier_has_a_distinct_wire_name() {
        let mut names: Vec<&str> = RepairId::all().iter().map(RepairId::as_str).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), total, "repair identifiers must be unique");
    }
}
