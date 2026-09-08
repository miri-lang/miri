// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shape checks over a file's own top-level statements.
//!
//! These run against the entry program only, before any body is checked. They
//! answer three questions the per-statement passes cannot, because each is a
//! property of the file as a whole: whether every top-level statement can
//! actually execute, whether two declarations claim the same name, and whether
//! an import is written more than once.

use crate::ast::expression::ImportPathKind;
use crate::ast::statement::{FunctionDeclarationData, StatementKind};
use crate::ast::{Expression, ExpressionKind, Program, Statement};
use crate::diagnostics::DiagnosticCode;
use crate::pipeline::is_script_body_statement;
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};

impl TypeChecker {
    /// Reports the top-level statements of `program` that cannot run or that
    /// redeclare a name already taken in the same file.
    pub(crate) fn check_top_level_shape(&mut self, program: &Program) {
        if declares_main(program) {
            self.report_unreachable_top_level_statements(program);
        }
        self.report_duplicate_function_declarations(program);
        self.report_duplicate_imports(program);
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

    /// A `use` repeated in one file imports nothing the first one did not, so
    /// the later line is dead text a reader still has to compare against the
    /// first before they can be sure of that.
    ///
    /// This is a warning rather than an error: the program means exactly what
    /// it would have meant with the repeat removed, and refusing to compile it
    /// would turn a tidying note into a failure.
    fn report_duplicate_imports(&mut self, program: &Program) {
        let mut seen: HashSet<String> = HashSet::new();
        for statement in top_level_statements(program) {
            let StatementKind::Use(path, alias) = &statement.node else {
                continue;
            };
            let Some(import) = import_identity(path, alias.as_deref()) else {
                continue;
            };
            if seen.insert(import.key) {
                continue;
            }
            self.report_warning(
                DiagnosticCode::ImpDuplicateImport,
                DiagnosticCode::ImpDuplicateImport.title().to_string(),
                format!(
                    "Duplicate import: '{}' is already imported in this file",
                    import.module
                ),
                statement.span,
                Some(
                    "an earlier 'use' above imports the same thing; remove this line.".to_string(),
                ),
            );
        }
    }
}

/// One `use` statement, reduced to what tells it apart from another.
struct ImportIdentity {
    /// The module path, as the diagnostic names it.
    module: String,
    /// Everything two `use` lines must share to be the same import.
    key: String,
}

/// What a repeated import has to match to count as the same import.
///
/// The module path alone is too coarse — two `use` lines can select different
/// names from one module and both be needed — so the selection and the module
/// alias are part of the key too. The selected names are sorted, because the
/// order they are written in changes nothing about what arrives.
///
/// A path this cannot read is left alone: `check_use` reports it as the invalid
/// import it is, and a second diagnostic about the same line would only repeat
/// that.
fn import_identity(path: &Expression, alias: Option<&Expression>) -> Option<ImportIdentity> {
    let (module, kind) = TypeChecker::extract_import_path_with_kind(path)?;
    let selection = match kind {
        ImportPathKind::Simple => String::new(),
        ImportPathKind::Wildcard => "*".to_string(),
        ImportPathKind::Multi(items) => {
            let mut names: Vec<String> = items
                .iter()
                .map(|(name, item_alias)| match item_alias {
                    Some(item_alias) => {
                        format!("{} as {}", rendered(name), rendered(item_alias))
                    }
                    None => rendered(name),
                })
                .collect();
            names.sort();
            names.join(",")
        }
    };
    let alias_text = alias.map(rendered).unwrap_or_default();
    let key = format!("{} {{{}}} as {}", module, selection, alias_text);
    Some(ImportIdentity { module, key })
}

/// The name an identifier expression carries, or its debug shape when it is not
/// one — enough to tell two selections apart, which is all the identity needs.
fn rendered(expression: &Expression) -> String {
    if let ExpressionKind::Identifier(name, _) = &expression.node {
        return name.clone();
    }
    format!("{:?}", expression.node)
}

/// The statements a file declares at its top level.
///
/// The parser can group consecutive top-level statements under a block, so a
/// pass that reads `program.body` alone would miss whatever landed inside one.
fn top_level_statements(program: &Program) -> impl Iterator<Item = &Statement> {
    program.body.iter().flat_map(|statement| {
        if let StatementKind::Block(statements) = &statement.node {
            statements.iter().collect::<Vec<_>>()
        } else {
            vec![statement]
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    /// The identity of every `use` statement in `source`, in the order written.
    fn identities(source: &str) -> Vec<String> {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        let program = match parser.parse() {
            Ok(program) => program,
            Err(error) => panic!("the fixture should parse: {:?}", error),
        };
        top_level_statements(&program)
            .filter_map(|statement| {
                let StatementKind::Use(path, alias) = &statement.node else {
                    return None;
                };
                import_identity(path, alias.as_deref()).map(|import| import.key)
            })
            .collect()
    }

    #[test]
    fn test_the_same_import_twice_has_one_identity() {
        let identities =
            identities("use system.testing.{assert_eq}\nuse system.testing.{assert_eq}\n");
        assert_eq!(identities.len(), 2);
        assert_eq!(identities[0], identities[1]);
    }

    #[test]
    fn test_selecting_different_names_from_one_module_differs() {
        let identities =
            identities("use system.testing.{assert_eq}\nuse system.testing.{assert_ne}\n");
        assert_eq!(identities.len(), 2);
        assert_ne!(identities[0], identities[1]);
    }

    #[test]
    fn test_the_order_names_are_selected_in_does_not_change_the_identity() {
        let identities =
            identities("use system.testing.{assert_eq, assert_ne}\nuse system.testing.{assert_ne, assert_eq}\n");
        assert_eq!(identities.len(), 2);
        assert_eq!(identities[0], identities[1]);
    }

    #[test]
    fn test_a_whole_module_and_a_selection_from_it_differ() {
        let identities = identities("use system.testing\nuse system.testing.{assert_eq}\n");
        assert_eq!(identities.len(), 2);
        assert_ne!(identities[0], identities[1]);
    }

    #[test]
    fn test_two_modules_differ() {
        let identities = identities("use system.testing\nuse system.math\n");
        assert_eq!(identities.len(), 2);
        assert_ne!(identities[0], identities[1]);
    }

    #[test]
    fn test_an_alias_is_part_of_the_identity() {
        let identities = identities("use system.testing as t\nuse system.testing\n");
        assert_eq!(identities.len(), 2);
        assert_ne!(identities[0], identities[1]);
    }
}
