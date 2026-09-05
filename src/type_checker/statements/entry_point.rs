// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shape checks over a file's own top-level statements.
//!
//! These run against the entry program only, before any body is checked. They
//! answer two questions the per-statement passes cannot, because both are
//! properties of the file as a whole: whether every top-level statement can
//! actually execute, and whether two declarations claim the same name.

use crate::ast::statement::{FunctionDeclarationData, StatementKind};
use crate::ast::{Program, Statement};
use crate::diagnostics::DiagnosticCode;
use crate::pipeline::is_script_body_statement;
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

impl TypeChecker {
    /// Reports the top-level statements of `program` that cannot run or that
    /// redeclare a name already taken in the same file.
    pub(crate) fn check_top_level_shape(&mut self, program: &Program) {
        if declares_main(program) {
            self.report_unreachable_top_level_statements(program);
        }
        self.report_duplicate_function_declarations(program);
    }

    /// A statement beside a declared `main` is never lowered, because
    /// script-mode wrapping — the only thing that would have given it a body to
    /// live in — is skipped for a file that names its own entry point.
    fn report_unreachable_top_level_statements(&mut self, program: &Program) {
        for statement in &program.body {
            if !is_script_body_statement(statement) {
                continue;
            }
            self.report_error_with_help(
                DiagnosticCode::TypTopLevelStatementBesideMain,
                "top-level statement will never run: this file declares 'main', so nothing \
                 executes it"
                    .to_string(),
                statement.span,
                "move the statement into 'main', or remove the 'main' declaration so the file \
                 runs as a script."
                    .to_string(),
            );
        }
    }

    /// Miri has no overloading, so a repeated top-level function name is a
    /// redeclaration whichever parameters it takes. The report lands on the
    /// later declaration, which is the one an author removes or renames.
    fn report_duplicate_function_declarations(&mut self, program: &Program) {
        let mut first_seen: HashMap<&str, ()> = HashMap::new();
        for statement in &program.body {
            let Some(declaration) = top_level_function(statement) else {
                continue;
            };
            if first_seen.insert(declaration.name.as_str(), ()).is_none() {
                continue;
            }
            self.report_error_with_help(
                DiagnosticCode::TypFunctionAlreadyDeclared,
                format!(
                    "Function '{}' is already declared in this file",
                    declaration.name
                ),
                // The name, not the whole statement: a declaration's span
                // starts at the file when the parser did not record one, and a
                // diagnostic a caller cannot locate is the defect this task
                // exists to remove.
                declaration.name_span,
                format!(
                    "an earlier declaration of '{}' appears above; rename one of them or remove \
                     this declaration.",
                    declaration.name
                ),
            );
        }
    }
}

fn declares_main(program: &Program) -> bool {
    program
        .body
        .iter()
        .filter_map(top_level_function)
        .any(|declaration| declaration.name == "main")
}

fn top_level_function(statement: &Statement) -> Option<&FunctionDeclarationData> {
    match &statement.node {
        StatementKind::FunctionDeclaration(declaration) => Some(declaration),
        _ => None,
    }
}
