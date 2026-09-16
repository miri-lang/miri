// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which method declarations name a drop hook, and the shapes `drop` may take.
//!
//! On a class or trait every method receives its instance implicitly, so
//! `fn drop(self)` and `fn drop()` declare the same hook. A struct spells its
//! receiver, so only `fn drop(self)` does. The name belongs to the hook: a
//! `drop` that takes arguments or is static would compile as an ordinary method
//! that nothing calls when the last reference goes, so it is refused.

use crate::ast::statement::FunctionDeclarationData;
use crate::ast::{Statement, StatementKind};
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::TypeChecker;

/// Returns true if a class or trait method statement is the drop hook,
/// `fn drop(self)` or `fn drop()`.
pub(crate) fn is_drop_method(stmt: &Statement) -> bool {
    matches!(&stmt.node, StatementKind::FunctionDeclaration(decl) if decl.is_drop_hook())
}

/// Returns true if a struct function statement is the drop hook `fn drop(self)`.
pub(crate) fn is_struct_drop_method(stmt: &Statement) -> bool {
    matches!(&stmt.node, StatementKind::FunctionDeclaration(decl) if decl.is_struct_drop_hook())
}

impl TypeChecker {
    /// Reports a class or trait method named `drop` that is not the hook, under
    /// `code` — the definition error of the type that declares it.
    pub(crate) fn check_drop_hook_shape(
        &mut self,
        decl: &FunctionDeclarationData,
        stmt: &Statement,
        code: DiagnosticCode,
    ) {
        if decl.name != crate::ast::statement::DROP_HOOK_NAME || decl.is_drop_hook() {
            return;
        }
        // Point at the method's own name when the declaration carries one.
        let span = if decl.name_span.end > decl.name_span.start {
            decl.name_span
        } else {
            stmt.span
        };
        self.report_error(
            code,
            "'drop' is the drop hook, which takes no arguments and runs on the instance \
             being released: declare it as 'fn drop(self)'"
                .to_string(),
            span,
        );
    }
}
