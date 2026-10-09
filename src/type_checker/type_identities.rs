// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which identity a declared type is registered under, and which one a name
//! written in a module resolves to.
//!
//! The spelling of an identity belongs to [`crate::ast::type_identity`]; this
//! module decides when a declaration is given a qualified one and remembers the
//! choice, so resolution and the stages after checking read it back rather
//! than deciding again.

use crate::ast::type_identity;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::ast::{Expression, ExpressionKind, MemberVisibility};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::diagnostics::shown_type_name;
use crate::type_checker::module_loader::PROGRAM_MODULE;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// The identity the type declared by `name_expr` with `visibility` in the
    /// module being checked is registered under, recorded so that the module's
    /// own names and the stages after checking resolve to it. `None` when the
    /// declaration names no type.
    pub(crate) fn register_declared_identity(
        &mut self,
        name_expr: &Expression,
        visibility: &MemberVisibility,
    ) -> Option<String> {
        let name = self.extract_type_name(name_expr).ok()?.to_string();
        let identity = self.identity_for(&name, visibility);
        if identity != name {
            self.modules
                .private_type_identities
                .entry(self.modules.current_module.clone())
                .or_default()
                .insert(name, identity.clone());
            self.record_qualified_name(name_expr, &identity);
        }
        Some(identity)
    }

    /// The identity a type the module being checked declares as `name` takes:
    /// qualified by the module's path when an imported module keeps it
    /// private, which no other module can name, and `name` itself otherwise.
    fn identity_for(&self, name: &str, visibility: &MemberVisibility) -> String {
        let module = self.modules.current_module.as_str();
        let is_imported = module != PROGRAM_MODULE;
        match visibility {
            MemberVisibility::Private if is_imported => type_identity::qualified(module, name),
            MemberVisibility::Private | MemberVisibility::Public | MemberVisibility::Protected => {
                name.to_string()
            }
        }
    }

    /// Remembers that `expr` names the type `identity`, when that is the
    /// qualified identity of a type a module keeps private.
    pub(crate) fn record_qualified_name(&mut self, expr: &Expression, identity: &str) {
        self.record_qualified_name_at(expr.id, identity);
    }

    /// Remembers that the identifier `expr_id` names the type whose metatype
    /// `ty` is, when a module keeps that type private: `Helper` in
    /// `Helper.make()`.
    pub(crate) fn record_type_named_by(&mut self, expr_id: usize, ty: &Type) {
        if let TypeKind::Meta(named) = &ty.kind {
            if let TypeKind::Custom(identity, _) = &named.kind {
                self.record_qualified_name_at(expr_id, identity);
            }
        }
    }

    fn record_qualified_name_at(&mut self, expr_id: usize, identity: &str) {
        // A synthesized expression carries id 0, which many share.
        if expr_id != 0 && type_identity::is_qualified(identity) {
            self.modules
                .qualified_type_names
                .insert(expr_id, identity.to_string());
            self.modules.qualified_name_recorded = true;
        }
    }

    /// The identity of the type the expression `expr` names: a declaration's
    /// name, or a type name written in a type or as the receiver of a static
    /// call. `None` when `expr` names no type.
    pub fn type_identity_named_by<'a>(&'a self, expr: &'a Expression) -> Option<&'a str> {
        match self.modules.qualified_type_names.get(&expr.id) {
            Some(identity) => Some(identity),
            None => self.extract_type_name(expr).ok(),
        }
    }

    /// The identity the type named by the type expression `expr` resolves to
    /// in the module being checked.
    pub(crate) fn extract_type_identity<'a>(
        &'a self,
        expr: &'a Expression,
    ) -> Result<&'a str, String> {
        self.extract_type_name(expr)
            .map(|name| self.written_type_identity(name))
    }

    /// The identity the type written as `name` resolves to in the module
    /// being checked.
    pub(crate) fn written_type_identity<'a>(&'a self, name: &'a str) -> &'a str {
        self.modules.type_identity(name)
    }

    /// The identity the type written as `name` resolves to where `context`
    /// is in scope: a type parameter of that name shadows a type the module
    /// keeps private.
    pub(crate) fn written_type_identity_in<'a>(
        &'a self,
        name: &'a str,
        context: &Context,
    ) -> &'a str {
        self.private_identity_in(name, context).unwrap_or(name)
    }

    /// The identity of the type the module being checked keeps private under
    /// `name`, unless a type parameter of that name is in scope in `context`.
    pub(crate) fn private_identity_in<'a>(
        &'a self,
        name: &str,
        context: &Context,
    ) -> Option<&'a str> {
        let identity = self.modules.private_identity(name)?;
        match context.resolve_type_definition(name) {
            Some(TypeDefinition::Generic(_)) => None,
            Some(
                TypeDefinition::Class(_)
                | TypeDefinition::Struct(_)
                | TypeDefinition::Enum(_)
                | TypeDefinition::Trait(_)
                | TypeDefinition::Alias(_),
            )
            | None => Some(identity),
        }
    }

    /// The type the type expression `expr` writes, with each name in it that
    /// checking resolved to a type a module keeps private spelled as that
    /// type's identity; `None` when it names no such type.
    ///
    /// A written type is its module's own spelling, and the stages after
    /// checking read it with no module to resolve its names in.
    pub fn qualified_written_type(&self, expr: &Expression) -> Option<&Type> {
        self.modules.qualified_written_types.get(&expr.id)
    }

    /// The identity a callee's declared signature resolved `expr` to in the
    /// module that declares it, when that is a type the module keeps private.
    pub(crate) fn declared_signature_identity(&self, expr: &Expression) -> Option<&str> {
        self.modules
            .qualified_type_names
            .get(&expr.id)
            .map(String::as_str)
    }

    /// Whether the module being checked may name the type `identity`: a type
    /// a module keeps private is reachable from that module alone.
    pub(crate) fn may_name_type(&self, identity: &str) -> bool {
        !type_identity::is_qualified(identity)
            || type_identity::is_declared_in(identity, &self.modules.current_module)
    }

    /// Remembers, for the type expression `expr` whose resolution `resolve`
    /// performs, the type it writes with each name that resolved to a type a
    /// module keeps private spelled as that type's identity. Only a type that
    /// names one is remembered, so the stages after checking pay one lookup for
    /// every other.
    pub(crate) fn resolving_written_type<T>(
        &mut self,
        expr: &Expression,
        resolve: impl FnOnce(&mut Self) -> T,
    ) -> T {
        let outer = std::mem::replace(&mut self.modules.qualified_name_recorded, false);
        let resolved = resolve(self);
        let recorded = self.modules.qualified_name_recorded;
        if recorded && expr.id != 0 {
            if let ExpressionKind::Type(written, _) = &expr.node {
                let qualified = self.qualified_type(written, expr.id);
                self.modules
                    .qualified_written_types
                    .insert(expr.id, qualified);
            }
        }
        self.modules.qualified_name_recorded = outer || recorded;
        resolved
    }

    /// `ty`, unless it names a type another module keeps private, which is
    /// refused at `span` as not visible.
    pub(crate) fn refuse_hidden_private_type(&mut self, ty: Type, span: Span) -> Type {
        if self.refuses_hidden_private_type(&ty, span) {
            return Type::new(TypeKind::Error, ty.span);
        }
        ty
    }

    /// Whether `ty` names a type another module keeps private, refusing it at
    /// `span` as not visible when it does.
    pub(crate) fn refuses_hidden_private_type(&mut self, ty: &Type, span: Span) -> bool {
        let Some(hidden) = self.hidden_private_type_in(&ty.kind) else {
            return false;
        };
        let message = format!("Type '{}' is not visible", shown_type_name(hidden));
        self.report_error(DiagnosticCode::TypNameNotVisible, message, span);
        true
    }

    /// The first type `kind` names, itself or in a component, that another
    /// module keeps private.
    fn hidden_private_type_in<'t>(&self, kind: &'t TypeKind) -> Option<&'t str> {
        if self.modules.qualified_type_names.is_empty() {
            return None;
        }
        if let TypeKind::Custom(name, _) = kind {
            if !self.may_name_type(name) {
                return Some(name);
            }
        }
        let (written, wrapped) = type_components(kind);
        written
            .into_iter()
            .find_map(|expr| {
                let ExpressionKind::Type(ty, _) = &expr.node else {
                    return None;
                };
                self.hidden_private_type_in(&ty.kind)
            })
            .or_else(|| wrapped.and_then(|ty| self.hidden_private_type_in(&ty.kind)))
    }

    /// `ty` with each name written in a component of it — a type argument, a
    /// closure's parameter or return — that checking resolved to a type a
    /// module keeps private spelled as that type's identity. A closure type
    /// read off a declaration carries the parameter types its module wrote.
    pub(crate) fn qualified_components(&self, ty: &Type) -> Type {
        if self.modules.qualified_type_names.is_empty() {
            return ty.clone();
        }
        self.qualified_type(ty, 0)
    }

    /// `ty`, written in the expression `id`, with its names qualified.
    fn qualified_type(&self, ty: &Type, id: usize) -> Type {
        Type::new(self.qualified_kind(&ty.kind, id), ty.span)
    }

    fn qualified_kind(&self, kind: &TypeKind, id: usize) -> TypeKind {
        let one = |expr: &Expression| Box::new(self.qualified_expression(expr));
        let inner = |ty: &Type| Box::new(self.qualified_type(ty, id));
        match kind {
            TypeKind::Custom(name, args) => {
                let name = self.modules.qualified_type_names.get(&id).unwrap_or(name);
                let args = args.as_deref().map(|args| self.qualified_expressions(args));
                TypeKind::Custom(name.clone(), args)
            }
            TypeKind::List(element) => TypeKind::List(one(element)),
            TypeKind::Set(element) => TypeKind::Set(one(element)),
            TypeKind::Future(element) => TypeKind::Future(one(element)),
            TypeKind::Array(element, size) => TypeKind::Array(one(element), size.clone()),
            TypeKind::Map(key, value) => TypeKind::Map(one(key), one(value)),
            TypeKind::Result(ok, err) => TypeKind::Result(one(ok), one(err)),
            TypeKind::Tuple(elements) => TypeKind::Tuple(self.qualified_expressions(elements)),
            TypeKind::OneOf(members) => TypeKind::OneOf(self.qualified_expressions(members)),
            TypeKind::Option(payload) => TypeKind::Option(inner(payload)),
            TypeKind::Meta(of) => TypeKind::Meta(inner(of)),
            TypeKind::Linear(of) => TypeKind::Linear(inner(of)),
            TypeKind::Function(function) => {
                TypeKind::Function(Box::new(self.qualified_function(function)))
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
            | TypeKind::Generic(..)
            | TypeKind::Void
            | TypeKind::Error => kind.clone(),
        }
    }

    /// A closure type with the names its parameters and return write
    /// qualified.
    pub(crate) fn qualified_function(&self, function: &FunctionTypeData) -> FunctionTypeData {
        let mut qualified = function.clone();
        for param in &mut qualified.params {
            *param.typ = self.qualified_expression(&param.typ);
        }
        if let Some(ret) = &mut qualified.return_type {
            **ret = self.qualified_expression(ret);
        }
        qualified
    }

    fn qualified_expressions(&self, exprs: &[Expression]) -> Vec<Expression> {
        exprs
            .iter()
            .map(|expr| self.qualified_expression(expr))
            .collect()
    }

    /// `expr`, a type argument or a component of a type, with the names it
    /// writes qualified; any other expression as it is.
    fn qualified_expression(&self, expr: &Expression) -> Expression {
        let mut qualified = expr.clone();
        if let ExpressionKind::Type(written, _) = &mut qualified.node {
            **written = self.qualified_type(written, expr.id);
        } else if let ExpressionKind::Identifier(name, _) = &mut qualified.node {
            if let Some(identity) = self.modules.qualified_type_names.get(&expr.id) {
                name.clone_from(identity);
            }
        }
        qualified
    }
}

/// The parts of a type that are types themselves: the expressions its
/// arguments, elements and a closure's signature are written in, and the type
/// an optional, a metatype or a linear type wraps.
fn type_components(kind: &TypeKind) -> (Vec<&Expression>, Option<&Type>) {
    match kind {
        TypeKind::Custom(_, args) => (args.iter().flatten().collect(), None),
        TypeKind::List(element)
        | TypeKind::Set(element)
        | TypeKind::Future(element)
        | TypeKind::Array(element, _) => (vec![element], None),
        TypeKind::Map(first, second) | TypeKind::Result(first, second) => {
            (vec![first, second], None)
        }
        TypeKind::Tuple(elements) | TypeKind::OneOf(elements) => (elements.iter().collect(), None),
        TypeKind::Option(inner) | TypeKind::Meta(inner) | TypeKind::Linear(inner) => {
            (Vec::new(), Some(inner))
        }
        TypeKind::Function(function) => {
            let params = function.params.iter().map(|param| &*param.typ);
            (
                params.chain(function.return_type.as_deref()).collect(),
                None,
            )
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
        | TypeKind::Generic(..)
        | TypeKind::Void
        | TypeKind::Error => (Vec::new(), None),
    }
}
