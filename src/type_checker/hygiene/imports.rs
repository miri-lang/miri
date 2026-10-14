// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Imports the file never needed.

use crate::ast::expression::{Expression, ExpressionKind, ImportPathKind};
use crate::ast::statement::StatementKind;
use crate::ast::{Program, Statement};
use crate::diagnostics::repair::RepairRequest;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
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
    ) -> Option<UnusedImport> {
        let (module, kind) = TypeChecker::extract_import_path_with_kind(path)?;
        if let Some(alias) = alias {
            let name = identifier(alias)?;
            return unread(vec![name], references).map(UnusedImport::WholeLine);
        }
        match kind {
            // What a wildcard brings in is whatever the module happens to
            // declare, so no name in the file points at the line itself.
            ImportPathKind::Wildcard => None,
            ImportPathKind::Multi(items) => self.unused_selection(&items, references),
            ImportPathKind::Simple => {
                let names = self.modules.module_declared_names.get(&module)?;
                unread(names.iter().cloned().collect(), references)
                    .map(|_| UnusedImport::WholeLine(module))
            }
        }
    }

    /// The names a selective import brings in that nothing reads, and
    /// whether the line has anything left on it worth keeping.
    ///
    /// Each selected name is asked about on its own, because selecting four
    /// names and using three of them leaves one line to edit and specific
    /// names to remove from it.
    fn unused_selection(
        &self,
        items: &[(Expression, Option<Box<Expression>>)],
        references: &References,
    ) -> Option<UnusedImport> {
        let mut unread = Vec::new();
        let mut kept = Vec::new();
        for (name, item_alias) in items {
            let bound = match item_alias {
                Some(item_alias) => identifier(item_alias)?,
                None => identifier(name)?,
            };
            let written = item_alias.as_ref().map_or(name.span, |alias| {
                Span::new(name.span.start, alias.span.end)
            });
            if references.contains(&bound) {
                kept.push(written);
            } else {
                unread.push(bound);
            }
        }
        if kept.is_empty() {
            return unread.into_iter().next().map(UnusedImport::WholeLine);
        }
        let first = items.first()?.0.span;
        let last = items
            .last()
            .map(|(name, alias)| alias.as_ref().map_or(name.span.end, |alias| alias.span.end))?;
        (!unread.is_empty()).then(|| UnusedImport::SelectedNames {
            unread,
            selection: Span::new(first.start, last),
            kept,
        })
    }

    /// Reports `unused`, once per name nothing reads, with the deletion its
    /// help names wherever the import is text a reader wrote.
    fn report_unused_import(&mut self, unused: &UnusedImport, statement: &Statement) {
        let repair = unused.repair(statement.span);
        for name in unused.names() {
            self.report_warning_with_repair(
                DiagnosticCode::ImpUnusedImport,
                DiagnosticCode::ImpUnusedImport.title().to_string(),
                format!("Unused import: '{}' is never used in this file", name),
                statement.span,
                Some(unused.help(name)),
                repair.clone(),
            );
        }
    }
}

/// An import nothing reads, and how much of the line goes with it.
enum UnusedImport {
    /// Nothing the line brings in is read, so the line itself is the edit.
    WholeLine(String),
    /// Names in a selection whose other names are read. Deleting the line
    /// would take those with it, so the edit is the selection, rewritten as
    /// the `kept` names written between its braces from `selection`.
    SelectedNames {
        unread: Vec<String>,
        selection: Span,
        kept: Vec<Span>,
    },
}

impl UnusedImport {
    /// The names to report, one diagnostic each.
    fn names(&self) -> Vec<&str> {
        match self {
            UnusedImport::WholeLine(name) => vec![name.as_str()],
            UnusedImport::SelectedNames { unread, .. } => {
                unread.iter().map(String::as_str).collect()
            }
        }
    }

    /// What to do about `name`.
    fn help(&self, name: &str) -> String {
        match self {
            UnusedImport::WholeLine(_) => {
                "nothing in this file reads what the import brings in; remove the line.".to_string()
            }
            UnusedImport::SelectedNames { .. } => format!(
                "the rest of the line is read; drop '{}' from the selection.",
                name
            ),
        }
    }

    /// The deletion the help names, for the `use` statement at `statement`.
    /// An import the compiler synthesized has no text behind its spans, so it
    /// carries the sentence alone.
    fn repair(&self, statement: Span) -> Option<RepairRequest> {
        match self {
            UnusedImport::WholeLine(_) => {
                (!statement.is_empty()).then_some(RepairRequest::RemoveImportLine {
                    start: statement.start,
                    end: statement.end,
                })
            }
            UnusedImport::SelectedNames {
                selection, kept, ..
            } => (!selection.is_empty() && kept.iter().all(|span| !span.is_empty())).then(|| {
                RepairRequest::DropSelectedImports {
                    start: selection.start,
                    end: selection.end,
                    kept: kept.iter().map(|span| (span.start, span.end)).collect(),
                }
            }),
        }
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
