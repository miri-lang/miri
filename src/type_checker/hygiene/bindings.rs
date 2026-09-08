// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Bindings nothing reads: locals and parameters.

use crate::ast::common::Parameter;
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::hygiene::is_deliberately_unread;
use crate::type_checker::hygiene::names::References;
use crate::type_checker::hygiene::walk::Contents;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reports each local whose name is read nowhere in the scope that declares
    /// it.
    ///
    /// The whole scope is searched rather than only the statements after the
    /// declaration, because a scope is not always read top to bottom: a
    /// function declared above a module-scope binding still reads it. The cost
    /// is that a name a second binding reuses in one scope looks read from
    /// both, which loses a report rather than inventing one.
    /// The file's own top level is skipped: a binding written there is part of
    /// the module's surface, reachable by everything that imports it, and this
    /// check sees one file. A private one is reported as the unused private
    /// declaration it is.
    pub(super) fn report_unused_locals(&mut self, contents: &Contents<'_>) {
        for scope in &contents.scopes {
            if scope.is_module {
                continue;
            }
            let references = References::of_statements(scope.statements);
            for statement in scope.statements {
                self.report_unused_locals_of(statement, &references);
            }
        }
    }

    fn report_unused_locals_of(&mut self, statement: &Statement, references: &References) {
        let StatementKind::Variable(declarations, _) = &statement.node else {
            return;
        };
        for declaration in declarations {
            let name = &declaration.name;
            if is_deliberately_unread(name) || references.contains(name) {
                continue;
            }
            self.report_warning(
                DiagnosticCode::TypUnusedLocal,
                DiagnosticCode::TypUnusedLocal.title().to_string(),
                format!("Unused local: '{}' is never read", name),
                statement.span,
                Some(format!(
                    "remove the binding, or name it '_{}' to say the value is not meant to be \
                     read.",
                    name
                )),
            );
        }
    }

    /// Reports each parameter of a function with a body that the body never
    /// reads.
    ///
    /// A declaration with no body — a trait method signature, an abstract
    /// method, a binding to a runtime or intrinsic function — has no unused
    /// parameters, because it has nothing that could have read them.
    pub(super) fn report_unused_parameters(&mut self, contents: &Contents<'_>) {
        for declaration in &contents.functions {
            let references = References::of_function_body(declaration);
            for parameter in &declaration.params {
                if is_read(&parameter.name, &references) {
                    continue;
                }
                self.report_unused_parameter(parameter, &declaration.name, declaration);
            }
        }
    }

    fn report_unused_parameter(
        &mut self,
        parameter: &Parameter,
        function: &str,
        declaration: &FunctionDeclarationData,
    ) {
        self.report_warning(
            DiagnosticCode::TypUnusedParameter,
            DiagnosticCode::TypUnusedParameter.title().to_string(),
            format!(
                "Unused parameter: '{}' is never read in the body of '{}'",
                parameter.name, function
            ),
            report_span(parameter, declaration),
            Some(format!(
                "remove the parameter and the arguments passed to it, or name it '_{}' to say \
                 this body has no use for the value.",
                parameter.name
            )),
        );
    }
}

/// A parameter counts as read when the declaration mentions its name, and when
/// it says outright that it is not meant to be.
///
/// `self` is the receiver every method has whether or not it names one, so a
/// body that never mentions it is an ordinary method rather than a mistake.
fn is_read(name: &str, references: &References) -> bool {
    name == "self" || is_deliberately_unread(name) || references.contains(name)
}

/// Where a report about a parameter points: at the parameter's own name, and at
/// the function's name when the parameter was built without source text.
fn report_span(parameter: &Parameter, declaration: &FunctionDeclarationData) -> Span {
    if parameter.name_span.is_empty() {
        declaration.name_span
    } else {
        parameter.name_span
    }
}
