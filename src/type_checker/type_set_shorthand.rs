// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The parameter shorthand for type sets: `fn sin(x Real) Real`.
//!
//! A type set written in a function's signature stands for one type parameter
//! the set bounds, and every use of the same set name in that signature is the
//! same parameter. So `fn clamp(x Real, lo Real, hi Real) Real` is
//! `fn clamp<T is Real>(x T, lo T, hi T) T`: a call binds `T` to one member,
//! and the result has the argument's type. Different set names are different
//! parameters, appended after any the function declares itself, in the order
//! their names first appear.
//!
//! The rewrite happens on the syntax tree before the module is checked, so every
//! later phase — the checker, monomorphization, the GPU backend — sees an
//! ordinary generic function. That needs to know which names are sets before
//! name resolution has run, so it reads the declarations directly: the sets a
//! module declares itself, and the public sets declared by the modules it
//! imports, filtered by a selective import's list.
//!
//! The parameter takes a name no source can spell — the set's name behind a
//! `'`, the way ML writes a type variable (`'Real`) — so it never collides with
//! a type the program declares, and
//! the body still reads the set's own name as the set — which, as the type of a
//! value, is refused there as everywhere else.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::ast::factory as ast;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::type_checker::statements::imports::module_search_locations;
use crate::type_checker::TypeChecker;

/// Marks a set's name as the name of the parameter it bounds. A leading mark,
/// because a trailing `'` is how an open call's inference slot is spelled.
pub(crate) const PARAMETER_MARK: char = '\'';

impl TypeChecker {
    /// Rewrites every top-level function whose signature names a type set into
    /// a generic function bounded by that set.
    pub(crate) fn desugar_type_set_parameters(&mut self, program: &mut Program) {
        let mut named = HashSet::new();
        for_each_function(program, &mut |declaration| {
            named.extend(signature_type_names(declaration));
        });
        if named.is_empty() {
            return;
        }
        let sets = self.visible_type_sets(program, &named);
        if sets.is_empty() {
            return;
        }
        for_each_function(program, &mut |declaration| {
            desugar_function(declaration, &sets)
        });
    }

    /// The type-set names a module can write in a signature: those it
    /// declares, and the public ones of each module it imports by name.
    ///
    /// The imported modules are read only when a signature names a type the
    /// module does not declare, which is the only way an imported set can be
    /// written in one; most modules name none.
    fn visible_type_sets(&mut self, program: &Program, named: &HashSet<String>) -> HashSet<String> {
        let mut sets: HashSet<String> = declared_type_sets(program, false).into_iter().collect();
        let declared: HashSet<String> = top_level_statements(program)
            .filter_map(|statement| self.declared_type_name(statement))
            .chain(declared_aliases(program))
            .collect();
        if named.iter().all(|name| declared.contains(name)) {
            return sets;
        }
        for statement in top_level_statements(program) {
            let StatementKind::Use(path, alias) = &statement.node else {
                continue;
            };
            if alias.is_some() {
                continue;
            }
            sets.extend(self.imported_type_sets(path));
        }
        sets
    }

    /// The type sets one `use` brings into scope under their own names.
    fn imported_type_sets(&mut self, path: &Expression) -> Vec<String> {
        let Some((path_str, kind)) = Self::extract_import_path_with_kind(path) else {
            return Vec::new();
        };
        let exported = self.module_type_sets(&path_str);
        match kind {
            ImportPathKind::Multi(items) => items
                .iter()
                .filter_map(|(item, alias)| {
                    let ExpressionKind::Identifier(name, _) = &item.node else {
                        return None;
                    };
                    if !exported.contains(name) {
                        return None;
                    }
                    match alias.as_deref().map(|alias| &alias.node) {
                        Some(ExpressionKind::Identifier(alias, _)) => Some(alias.clone()),
                        _ => Some(name.clone()),
                    }
                })
                .collect(),
            ImportPathKind::Simple | ImportPathKind::Wildcard => exported,
        }
    }

    /// The public type sets the module at `path_str` declares, read from its
    /// source once and remembered.
    fn module_type_sets(&mut self, path_str: &str) -> Vec<String> {
        let source_dir = self.modules.source_dir.clone();
        let Some(file) = module_search_locations(path_str, source_dir.as_deref())
            .into_iter()
            .map(|(_, location)| location)
            .find(|location| location.exists())
        else {
            return Vec::new();
        };
        if let Some(cached) = self.type_set_exports.get(&file) {
            return cached.clone();
        }
        let exported = read_public_type_sets(&file);
        self.type_set_exports.insert(file, exported.clone());
        exported
    }
}

/// The public type sets the module source at `file` declares; none when it
/// cannot be read or parsed, which the import itself goes on to report.
fn read_public_type_sets(file: &PathBuf) -> Vec<String> {
    let Ok(source) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let mut lexer = Lexer::new(&source);
    let mut parser = Parser::new(&mut lexer, &source);
    match parser.parse() {
        Ok(program) => declared_type_sets(&program, true),
        Err(_) => Vec::new(),
    }
}

/// The names of the type sets `program` declares, only the public ones when
/// `public_only`.
fn declared_type_sets(program: &Program, public_only: bool) -> Vec<String> {
    let mut sets = Vec::new();
    for statement in top_level_statements(program) {
        let StatementKind::Type(declarations, visibility) = &statement.node else {
            continue;
        };
        if public_only && *visibility != MemberVisibility::Public {
            continue;
        }
        for declaration in declarations {
            if let ExpressionKind::TypeDeclaration(name, _, TypeDeclarationKind::Is, Some(target)) =
                &declaration.node
            {
                if is_type_set_expression(target) {
                    if let ExpressionKind::Identifier(name, _) = &name.node {
                        sets.push(name.clone());
                    }
                }
            }
        }
    }
    sets
}

/// The names of every `type` declaration `program` makes, sets included.
fn declared_aliases(program: &Program) -> impl Iterator<Item = String> + '_ {
    top_level_statements(program).flat_map(|statement| {
        let StatementKind::Type(declarations, _) = &statement.node else {
            return Vec::new();
        };
        declarations
            .iter()
            .filter_map(|declaration| match &declaration.node {
                ExpressionKind::TypeDeclaration(name, _, _, _) => match &name.node {
                    ExpressionKind::Identifier(name, _) => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    })
}

/// The parts of a top-level function declaration the shorthand reads and
/// rewrites: an ordinary function and an intrinsic alike.
struct Signature<'a> {
    generics: &'a mut Option<Vec<Expression>>,
    params: &'a mut Vec<Parameter>,
    return_type: &'a mut Option<Box<Expression>>,
}

impl<'a> Signature<'a> {
    fn of(statement: &'a mut StatementKind) -> Option<Self> {
        match statement {
            StatementKind::FunctionDeclaration(declaration) => Some(Signature {
                generics: &mut declaration.generics,
                params: &mut declaration.params,
                return_type: &mut declaration.return_type,
            }),
            StatementKind::IntrinsicFunctionDeclaration(_, generics, params, return_type, _) => {
                Some(Signature {
                    generics,
                    params,
                    return_type,
                })
            }
            _ => None,
        }
    }
}

/// Calls `visit` on every function declared at a module's top level.
fn for_each_function(program: &mut Program, visit: &mut impl FnMut(&mut Signature)) {
    for statement in &mut program.body {
        if let StatementKind::Block(statements) = &mut statement.node {
            for inner in statements {
                if let Some(mut signature) = Signature::of(&mut inner.node) {
                    visit(&mut signature);
                }
            }
        } else if let Some(mut signature) = Signature::of(&mut statement.node) {
            visit(&mut signature);
        }
    }
}

/// The bare type names a function's signature writes, other than its own
/// type parameters.
fn signature_type_names(declaration: &mut Signature) -> HashSet<String> {
    let parameters = declared_parameters(declaration);
    let mut names = HashSet::new();
    for_each_signature_type(declaration, &mut |name| {
        if !parameters.contains(name.as_str()) {
            names.insert(name.clone());
        }
    });
    names
}

/// The names of the type parameters a function declares.
fn declared_parameters(declaration: &Signature) -> HashSet<String> {
    declaration
        .generics
        .iter()
        .flatten()
        .filter_map(generic_parameter_name)
        .collect()
}

/// Every statement at a module's top level, a block's statements included.
fn top_level_statements(program: &Program) -> impl Iterator<Item = &Statement> {
    program
        .body
        .iter()
        .flat_map(|statement| match &statement.node {
            StatementKind::Block(statements) => statements.iter().collect::<Vec<_>>(),
            _ => vec![statement],
        })
}

/// Whether a `type ... is` target spells a set rather than a single type.
fn is_type_set_expression(target: &Expression) -> bool {
    matches!(&target.node, ExpressionKind::Type(written, _) if matches!(written.kind, TypeKind::OneOf(_)))
}

/// Gives a function whose signature names a set in `sets` one type parameter
/// per set name, and points the signature at it.
fn desugar_function(declaration: &mut Signature, sets: &HashSet<String>) {
    let declared = declared_parameters(declaration);
    let mut used: Vec<String> = Vec::new();
    for_each_signature_type(declaration, &mut |name| {
        if sets.contains(name.as_str()) && !declared.contains(name.as_str()) && !used.contains(name)
        {
            used.push(name.clone());
        }
    });
    if used.is_empty() {
        return;
    }
    let parameters: HashMap<String, String> = used
        .iter()
        .map(|set| (set.clone(), parameter_name(set)))
        .collect();
    for_each_signature_type(declaration, &mut |name| {
        if let Some(parameter) = parameters.get(name.as_str()) {
            *name = parameter.clone();
        }
    });
    let generics = declaration.generics.get_or_insert_with(Vec::new);
    for set in used {
        generics.push(ast::generic_type_expression(
            ast::identifier(&parameter_name(&set)),
            Some(Box::new(ast::type_expr_non_null(ast::make_type(
                TypeKind::Custom(set, None),
            )))),
            TypeDeclarationKind::Is,
        ));
    }
}

/// The name of the parameter the shorthand introduces for `set`.
fn parameter_name(set: &str) -> String {
    format!("{PARAMETER_MARK}{set}")
}

/// The name a generic parameter declaration introduces.
fn generic_parameter_name(generic: &Expression) -> Option<String> {
    let ExpressionKind::GenericType(name, _, _) = &generic.node else {
        return None;
    };
    match &name.node {
        ExpressionKind::Identifier(name, _) => Some(name.clone()),
        _ => None,
    }
}

/// Calls `visit` on every bare type name in a function's parameter and return
/// types, in the order they are written.
fn for_each_signature_type(declaration: &mut Signature, visit: &mut impl FnMut(&mut String)) {
    for parameter in declaration.params.iter_mut() {
        visit_type_names(&mut parameter.typ, visit);
    }
    if let Some(return_type) = declaration.return_type.as_mut() {
        visit_type_names(return_type, visit);
    }
}

/// Calls `visit` on every bare type name inside a written type, its type
/// arguments and element types included.
fn visit_type_names(expression: &mut Expression, visit: &mut impl FnMut(&mut String)) {
    match &mut expression.node {
        ExpressionKind::Type(written, _) => visit_kind_names(written, visit),
        ExpressionKind::Identifier(name, _) => visit(name),
        _ => {}
    }
}

fn visit_kind_names(written: &mut Type, visit: &mut impl FnMut(&mut String)) {
    match &mut written.kind {
        TypeKind::Custom(name, arguments) => {
            if arguments.is_none() {
                visit(name);
            }
            for argument in arguments.iter_mut().flatten() {
                visit_type_names(argument, visit);
            }
        }
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
            visit_type_names(element, visit)
        }
        TypeKind::Array(element, _) => visit_type_names(element, visit),
        TypeKind::Map(first, second) | TypeKind::Result(first, second) => {
            visit_type_names(first, visit);
            visit_type_names(second, visit);
        }
        TypeKind::Tuple(elements) | TypeKind::OneOf(elements) => elements
            .iter_mut()
            .for_each(|element| visit_type_names(element, visit)),
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            visit_kind_names(inner, visit)
        }
        TypeKind::Function(function) => {
            for parameter in &mut function.params {
                visit_type_names(&mut parameter.typ, visit);
            }
            if let Some(return_type) = &mut function.return_type {
                visit_type_names(return_type, visit);
            }
        }
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Generic(_, _, _)
        | TypeKind::Void
        | TypeKind::Error => {}
    }
}
