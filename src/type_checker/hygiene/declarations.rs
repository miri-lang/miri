// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Private declarations nothing in the file uses.

use crate::ast::common::MemberVisibility;
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::hygiene::names::References;
use crate::type_checker::hygiene::walk::{Contents, Declaration};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reports each `private` declaration whose name the file never uses.
    ///
    /// `private` says the name is reachable from nowhere else, so a file that
    /// does not use it holds a declaration nothing can reach. A public
    /// declaration is never reported: an exported name is used by whoever
    /// imports the module, and this check sees one file.
    pub(super) fn report_unused_private_declarations(
        &mut self,
        contents: &Contents<'_>,
        references: &References,
    ) {
        for declaration in &contents.declarations {
            if is_reachable(declaration, references) {
                continue;
            }
            self.report_unused_private_declaration(declaration);
        }
    }

    fn report_unused_private_declaration(&mut self, declaration: &Declaration<'_>) {
        self.report_warning(
            DiagnosticCode::TypUnusedPrivateDeclaration,
            DiagnosticCode::TypUnusedPrivateDeclaration
                .title()
                .to_string(),
            format!(
                "Unused private declaration: '{}' is never used in this file",
                declaration.name
            ),
            declaration.span,
            Some(format!(
                "remove the {}, or declare it 'public' if it is meant to be part of this \
                 module's surface.",
                declaration.noun
            )),
        );
    }
}

/// A declaration is left alone unless it is private and the file never names it.
fn is_reachable(declaration: &Declaration<'_>, references: &References) -> bool {
    !matches!(declaration.visibility, MemberVisibility::Private)
        || references.contains(declaration.name)
}
