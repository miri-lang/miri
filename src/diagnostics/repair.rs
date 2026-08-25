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
}

impl RepairId {
    /// The wire string tooling matches on.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LetToVar => "let-to-var",
            Self::AddImport => "add-import",
            Self::DropExtraArguments => "drop-extra-arguments",
        }
    }

    /// Every repair identifier, in declaration order.
    pub fn all() -> &'static [RepairId] {
        &[Self::LetToVar, Self::AddImport, Self::DropExtraArguments]
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
    LetToVar { keyword_start: usize },
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
}

/// The `let` keyword, and the `var` that replaces it. Equal length is a
/// coincidence of the language, not something the edit relies on.
const LET_KEYWORD: &str = "let";

impl RepairRequest {
    /// The stable identifier for this request's shape.
    pub fn id(&self) -> RepairId {
        match self {
            Self::LetToVar { .. } => RepairId::LetToVar,
            Self::AddImport { .. } => RepairId::AddImport,
            Self::DropExtraArguments { .. } => RepairId::DropExtraArguments,
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
            Self::LetToVar { keyword_start } => {
                Self::project_let_to_var(path, source, *keyword_start)
            }
            Self::AddImport { module, name } => {
                Self::project_add_import(path, source, module, name)
            }
            Self::DropExtraArguments { start, end } => {
                Self::project_drop_extra_arguments(path, source, *start, *end)
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
        let request = RepairRequest::LetToVar { keyword_start: 0 };

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
