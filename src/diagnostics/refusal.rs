// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Refusal gate for applying repairs based on their safety classification.
//!
//! Determines which repairs the `--apply` mode should refuse without writing files.

use crate::diagnostics::json::JsonDiagnostic;
use crate::diagnostics::{DiagnosticCode, FixSafety, RepairRequest};

/// Represents a repair that cannot be applied without explicit user approval.
#[derive(Debug, Clone)]
pub struct RefusedRepair {
    /// The diagnostic code that names this repair.
    pub code: DiagnosticCode,
    /// The safety level that caused the refusal.
    pub fix_safety: FixSafety,
}

/// Compute the safety level of a repair.
///
/// All repair types carry an inherent safety classification that is
/// independent of the code being repaired, so this function always
/// returns a concrete level.
pub fn repair_fix_safety(repair: &RepairRequest) -> FixSafety {
    match repair {
        RepairRequest::LetToVar {
            keyword_start: _,
            module_scope,
            is_public,
        } => {
            // Only public module-scope bindings are api-changing.
            // Private module-scope bindings and all function-local bindings are local-edit.
            if *module_scope && *is_public {
                FixSafety::ApiChanging
            } else {
                FixSafety::LocalEdit
            }
        }
        RepairRequest::AddImport { module: _, name: _ } => {
            // Adding an import is a local edit confined to the current file.
            FixSafety::LocalEdit
        }
        RepairRequest::DropExtraArguments { start: _, end: _ } => {
            // Dropping extra arguments is a local edit to the current function.
            FixSafety::LocalEdit
        }
        RepairRequest::ColonAnnotation {
            colon_start: _,
            colon_end: _,
        } => {
            // Removing a type annotation colon is a local edit.
            FixSafety::LocalEdit
        }
        RepairRequest::ArrowReturnType {
            arrow_start: _,
            arrow_end: _,
        } => {
            // Removing a return type arrow is a local edit.
            FixSafety::LocalEdit
        }
        RepairRequest::LetMutToVar {
            keyword_start: _,
            mut_end: _,
        } => {
            // Replacing let mut with var is a local edit.
            FixSafety::LocalEdit
        }
        RepairRequest::NullToNone {
            spelling_start: _,
            spelling_end: _,
        } => {
            // Replacing null with None is a local edit.
            FixSafety::LocalEdit
        }
        RepairRequest::PrintlnBang { bang_start: _ } => {
            // Removing the macro bang is a local edit.
            FixSafety::LocalEdit
        }
    }
}

/// Determine which diagnostics should be refused in --apply mode.
///
/// A repair is refused if its effective fix-safety level is in the set of
/// risky labels (ApiChanging, TargetChanging, RequiresHumanReview) and the
/// user has not passed `--allow-risky`.
///
/// When a diagnostic carries a repair but its safety cannot be established
/// (missing code, invalid code, missing fix_safety, or unparseable fix_safety),
/// it is refused unless `--allow-risky` is set. This is a fail-closed policy:
/// we do not apply repairs whose safety we cannot verify.
///
/// This is a pure function with no I/O, so it can be unit-tested independently.
pub fn compute_refused_repairs<'a, I>(diagnostics: I, allow_risky: bool) -> Vec<RefusedRepair>
where
    I: IntoIterator<Item = &'a JsonDiagnostic>,
{
    // Nothing is refused when the caller has accepted the risk, so the
    // classification below only has to answer the question it is asked.
    if allow_risky {
        return Vec::new();
    }

    diagnostics
        .into_iter()
        .filter(|diagnostic| diagnostic.repair.is_some())
        .filter_map(|diagnostic| {
            // A repair is applied only on a positive answer. An unnameable
            // code or an unreadable label is not that answer: it leaves the
            // bar the repair has to clear unknown, so the repair does not
            // clear it and the label reported is the one that withholds it.
            let code = diagnostic
                .code
                .as_deref()
                .and_then(|code| code.parse::<DiagnosticCode>().ok());
            let fix_safety = diagnostic
                .fix_safety
                .as_deref()
                .and_then(|label| label.parse::<FixSafety>().ok());

            match (code, fix_safety) {
                (Some(_), Some(fix_safety)) if fix_safety.is_auto_applicable() => None,
                (code, fix_safety) => Some(RefusedRepair {
                    code: code.unwrap_or(DiagnosticCode::BldUnknownDiagnosticCode),
                    fix_safety: fix_safety.unwrap_or(FixSafety::RequiresHumanReview),
                }),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_public_module_scope_let_to_var_is_api_changing() {
        let repair = RepairRequest::LetToVar {
            keyword_start: 0,
            module_scope: true,
            is_public: true,
        };
        assert_eq!(repair_fix_safety(&repair), FixSafety::ApiChanging);
    }

    #[test]
    fn test_private_module_scope_let_to_var_is_local_edit() {
        let repair = RepairRequest::LetToVar {
            keyword_start: 0,
            module_scope: true,
            is_public: false,
        };
        assert_eq!(repair_fix_safety(&repair), FixSafety::LocalEdit);
    }

    #[test]
    fn test_function_local_let_to_var_is_local_edit() {
        let repair = RepairRequest::LetToVar {
            keyword_start: 0,
            module_scope: false,
            is_public: false,
        };
        assert_eq!(repair_fix_safety(&repair), FixSafety::LocalEdit);
    }

    #[test]
    fn test_function_local_public_let_to_var_is_local_edit() {
        // Public flag is ignored for function-local bindings
        let repair = RepairRequest::LetToVar {
            keyword_start: 0,
            module_scope: false,
            is_public: true,
        };
        assert_eq!(repair_fix_safety(&repair), FixSafety::LocalEdit);
    }

    #[test]
    fn test_add_import_is_local_edit() {
        let repair = RepairRequest::AddImport {
            module: "system.math".to_string(),
            name: "sqrt".to_string(),
        };
        assert_eq!(repair_fix_safety(&repair), FixSafety::LocalEdit);
    }

    #[test]
    fn test_drop_extra_arguments_is_local_edit() {
        let repair = RepairRequest::DropExtraArguments { start: 10, end: 20 };
        assert_eq!(repair_fix_safety(&repair), FixSafety::LocalEdit);
    }

    #[test]
    fn test_missing_code_is_refused() {
        use crate::diagnostics::json::JsonRepair;
        let diag = JsonDiagnostic {
            severity: "error".to_string(),
            code: None,
            message: "test error".to_string(),
            path: None,
            line: None,
            column: None,
            length: None,
            expected: None,
            actual: None,
            help: None,
            fix_safety: Some("local-edit".to_string()),
            repair: Some(JsonRepair {
                id: "test".to_string(),
                summary: "test repair".to_string(),
                edits: vec![],
            }),
            related: vec![],
            preexisting: None,
        };
        let refused = compute_refused_repairs(&[diag], false);
        assert_eq!(
            refused.len(),
            1,
            "diagnostic with missing code must be refused"
        );
    }

    #[test]
    fn test_unparseable_code_is_refused() {
        use crate::diagnostics::json::JsonRepair;
        let diag = JsonDiagnostic {
            severity: "error".to_string(),
            code: Some("INVALID_CODE".to_string()),
            message: "test error".to_string(),
            path: None,
            line: None,
            column: None,
            length: None,
            expected: None,
            actual: None,
            help: None,
            fix_safety: Some("local-edit".to_string()),
            repair: Some(JsonRepair {
                id: "test".to_string(),
                summary: "test repair".to_string(),
                edits: vec![],
            }),
            related: vec![],
            preexisting: None,
        };
        let refused = compute_refused_repairs(&[diag], false);
        assert_eq!(
            refused.len(),
            1,
            "diagnostic with unparseable code must be refused"
        );
    }

    #[test]
    fn test_missing_fix_safety_is_refused() {
        use crate::diagnostics::json::JsonRepair;
        let diag = JsonDiagnostic {
            severity: "error".to_string(),
            code: Some("MER_BLD_002".to_string()),
            message: "test error".to_string(),
            path: None,
            line: None,
            column: None,
            length: None,
            expected: None,
            actual: None,
            help: None,
            fix_safety: None,
            repair: Some(JsonRepair {
                id: "test".to_string(),
                summary: "test repair".to_string(),
                edits: vec![],
            }),
            related: vec![],
            preexisting: None,
        };
        let refused = compute_refused_repairs(&[diag], false);
        assert_eq!(
            refused.len(),
            1,
            "diagnostic with missing fix_safety must be refused"
        );
    }

    #[test]
    fn test_unparseable_fix_safety_is_refused() {
        use crate::diagnostics::json::JsonRepair;
        let diag = JsonDiagnostic {
            severity: "error".to_string(),
            code: Some("MER_BLD_002".to_string()),
            message: "test error".to_string(),
            path: None,
            line: None,
            column: None,
            length: None,
            expected: None,
            actual: None,
            help: None,
            fix_safety: Some("not-a-real-level".to_string()),
            repair: Some(JsonRepair {
                id: "test".to_string(),
                summary: "test repair".to_string(),
                edits: vec![],
            }),
            related: vec![],
            preexisting: None,
        };
        let refused = compute_refused_repairs(&[diag], false);
        assert_eq!(
            refused.len(),
            1,
            "diagnostic with unparseable fix_safety must be refused"
        );
    }

    #[test]
    fn test_allow_risky_overrides_failed_parse() {
        use crate::diagnostics::json::JsonRepair;
        let diag = JsonDiagnostic {
            severity: "error".to_string(),
            code: None,
            message: "test error".to_string(),
            path: None,
            line: None,
            column: None,
            length: None,
            expected: None,
            actual: None,
            help: None,
            fix_safety: Some("local-edit".to_string()),
            repair: Some(JsonRepair {
                id: "test".to_string(),
                summary: "test repair".to_string(),
                edits: vec![],
            }),
            related: vec![],
            preexisting: None,
        };
        let refused = compute_refused_repairs(&[diag], true);
        assert_eq!(
            refused.len(),
            0,
            "--allow-risky should override unknown safety"
        );
    }
}
