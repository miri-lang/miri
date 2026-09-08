// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Imports the file never needed.

use crate::ast::expression::{Expression, ExpressionKind, ImportPathKind};
use crate::ast::statement::StatementKind;
use crate::ast::{Program, Statement};
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::hygiene::names::References;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Reports each `use` in the file whose names nothing in the file reads.
    ///
    /// A file that declares nothing is left alone. Its imports are what it is
    /// for: everything that imports it reads them, and nothing in the file
    /// itself ever could. That is the spelling for an import kept on purpose —
    /// a file of nothing but `use` lines re-exports them.
    pub(super) fn report_unused_imports(&mut self, program: &Program, references: &References) {
        if declares_nothing(program) {
            return;
        }
        for statement in top_level_statements(program) {
            let StatementKind::Use(path, alias) = &statement.node else {
                continue;
            };
            let Some(unused) = self.unused_import(path, alias.as_deref(), references) else {
                continue;
            };
            self.report_unused_import(&unused, statement);
        }
    }

    /// The name this `use` binds that nothing reads, when it binds one and
    /// nothing reads any of them.
    ///
    /// `None` covers three different situations, all of which must stay quiet:
    /// the path did not parse as an import, the module is not one whose
    /// contents are known, and the import is used.
    fn unused_import(
        &self,
        path: &Expression,
        alias: Option<&Expression>,
        references: &References,
    ) -> Option<String> {
        let (module, kind) = TypeChecker::extract_import_path_with_kind(path)?;
        if let Some(alias) = alias {
            let name = identifier(alias)?;
            return unread(vec![name], references);
        }
        match kind {
            // What a wildcard brings in is whatever the module happens to
            // declare, so no name in the file points at the line itself.
            ImportPathKind::Wildcard => None,
            ImportPathKind::Multi(items) => {
                let selected = self.selected_names(&items, references)?;
                Some(selected)
            }
            ImportPathKind::Simple => {
                let names = self.modules.module_declared_names.get(&module)?;
                unread(names.iter().cloned().collect(), references).map(|_| module)
            }
        }
    }

    /// The first name a selective import brings in that nothing reads.
    ///
    /// Each selected name is asked about on its own, because selecting four
    /// names and using three of them leaves one line to edit and a specific
    /// name to remove from it.
    fn selected_names(
        &self,
        items: &[(Expression, Option<Box<Expression>>)],
        references: &References,
    ) -> Option<String> {
        for (name, item_alias) in items {
            let bound = match item_alias {
                Some(item_alias) => identifier(item_alias)?,
                None => identifier(name)?,
            };
            if !references.contains(&bound) {
                return Some(bound);
            }
        }
        None
    }

    fn report_unused_import(&mut self, name: &str, statement: &Statement) {
        self.report_warning(
            DiagnosticCode::ImpUnusedImport,
            DiagnosticCode::ImpUnusedImport.title().to_string(),
            format!("Unused import: '{}' is never used in this file", name),
            statement.span,
            Some(
                "nothing in this file reads what the import brings in; remove the line."
                    .to_string(),
            ),
        );
    }
}

/// The module path itself, when nothing reads any of the names it brought in.
fn unread(names: Vec<String>, references: &References) -> Option<String> {
    if names.iter().any(|name| references.contains(name)) {
        return None;
    }
    names.into_iter().next()
}

/// The name an identifier expression carries. A path this cannot read is left
/// alone: `check_use` reports it as the invalid import it is.
fn identifier(expression: &Expression) -> Option<String> {
    match &expression.node {
        ExpressionKind::Identifier(name, _) => Some(name.clone()),
        ExpressionKind::Literal(_)
        | ExpressionKind::Binary(..)
        | ExpressionKind::Logical(..)
        | ExpressionKind::Unary(..)
        | ExpressionKind::Assignment(..)
        | ExpressionKind::Conditional(..)
        | ExpressionKind::Range(..)
        | ExpressionKind::Guard(..)
        | ExpressionKind::Member(..)
        | ExpressionKind::Index(..)
        | ExpressionKind::Call(..)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::Type(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..)
        | ExpressionKind::EnumValue(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::Lambda(..)
        | ExpressionKind::List(..)
        | ExpressionKind::Array(..)
        | ExpressionKind::Map(..)
        | ExpressionKind::Tuple(..)
        | ExpressionKind::Set(..)
        | ExpressionKind::Match(..)
        | ExpressionKind::FormattedString(..)
        | ExpressionKind::NamedArgument(..)
        | ExpressionKind::Super
        | ExpressionKind::Block(..)
        | ExpressionKind::Cast(..) => None,
    }
}

/// True when the file's top level is imports and nothing else.
fn declares_nothing(program: &Program) -> bool {
    top_level_statements(program).all(|statement| {
        matches!(
            statement.node,
            StatementKind::Use(_, _) | StatementKind::Empty
        )
    })
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
