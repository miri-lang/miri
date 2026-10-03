// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which method declarations name a drop hook, and the shapes `drop` may take.
//!
//! On a class or trait every method receives its instance implicitly, so
//! `fn drop(self)` and `fn drop()` declare the same hook. The name belongs to
//! the hook: a
//! `drop` that takes arguments or is static would compile as an ordinary method
//! that nothing calls when the last reference goes, so it is refused.

use crate::ast::statement::FunctionDeclarationData;
use crate::ast::Statement;
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::TypeChecker;

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
