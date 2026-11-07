// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! One method per name in a class, enum or trait.
//!
//! Miri has no overloading, so a type's methods are keyed by name: a second
//! declaration of a name would replace the first, and a call written against
//! either reaches code generation as two definitions of one symbol. A second
//! declaration is refused where it is written, naming the first, which is
//! kept. A static and an instance method share the name space too; that pair
//! is refused with the static-method restriction, which says why the two
//! cannot be told apart at a call.
//!
//! A subclass redeclaring a method of a class it extends, or a class
//! declaring a method a trait it implements declares, writes the name in a
//! different type, so neither meets the first declaration here.

use std::collections::HashMap;

use crate::ast::statement::FunctionDeclarationData;
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::RelatedNote;
use crate::error::syntax::Span;
use crate::type_checker::TypeChecker;

/// The methods one type has declared so far, by name.
#[derive(Default)]
pub(crate) struct DeclaredMethods {
    first: HashMap<String, FirstDeclaration>,
}

/// Where a method name was first declared, and whether that method is static.
struct FirstDeclaration {
    span: Span,
    is_static: bool,
}

impl TypeChecker {
    /// Refuse `decl`, a method of a type of `owner_kind` (`class`, `enum`,
    /// `trait`), when `declared` already holds its name, and record it
    /// otherwise. Returns whether it was refused, so the caller keeps the
    /// first declaration rather than replacing it.
    pub(crate) fn refuse_redeclared_method(
        &mut self,
        declared: &mut DeclaredMethods,
        decl: &FunctionDeclarationData,
        statement_span: Span,
        owner_kind: &str,
    ) -> bool {
        // Point at the method's own name. Programmatically built declarations
        // carry no name span, so they fall back to the statement.
        let span = if decl.name_span.is_empty() {
            statement_span
        } else {
            decl.name_span
        };
        let Some(first) = declared.first.get(&decl.name) else {
            let first = FirstDeclaration {
                span,
                is_static: decl.properties.is_static,
            };
            declared.first.insert(decl.name.clone(), first);
            return false;
        };
        let name = &decl.name;
        if first.is_static != decl.properties.is_static {
            self.report_error(
                DiagnosticCode::TypStaticMethodRestriction,
                format!("Method '{name}' cannot be both instance and static method"),
                span,
            );
            return true;
        }
        let note = RelatedNote::at(
            format!("the first declaration of '{name}' is here"),
            first.span,
        );
        self.report_error_with_related(
            DiagnosticCode::TypFunctionAlreadyDeclared,
            format!("Method '{name}' is already declared in this {owner_kind}"),
            span,
            Some(format!(
                "Miri has no overloading: a {owner_kind} declares each method name once; rename \
                 one of them or remove this declaration."
            )),
            vec![note],
            None,
        );
        true
    }
}
