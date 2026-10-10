// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Refusing a store that puts an object back into one of its own fields.
//!
//! Memory is reference counted, and reference counting never frees a cycle:
//! once an object holds a reference to itself, its count cannot reach zero.
//! So `self.on_tick = fn() int: self.bump()` — a closure whose environment
//! holds `self`, stored in a field of `self` — leaks both the object and the
//! closure, as does `self.next = Some(self)`.
//!
//! The check sees the direct form only: a store into a place rooted at a
//! class or trait binding (`x.f`, `x.f.g`, `x.items[i]`) — a binding whose
//! copies share one object — whose stored value is
//! that same binding, a closure capturing it, or a variant, constructor call
//! or literal holding one of those. A cycle closed through another name or
//! another object (`let u = x` then `x.f = fn(): u.n`, or `a.b = b` then
//! `b.a = a`) is not seen, and still leaks.
//!
//! TODO: those indirect cycles need weak references or a cycle collector to
//! be freed; refusing them would take flow tracking through aliases and
//! across objects, which this syntactic check does not attempt.

use crate::ast::captures::collect_lambda_captures;
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::runtime_fns::cow_fn;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::TypeChecker;

/// How a stored value holds the object it is stored into.
#[derive(Clone, Copy)]
enum Holding {
    /// The value is the object itself.
    Itself,
    /// The value is, or holds, a closure capturing the object.
    Captured,
    /// The value is a variant, constructed value or literal holding the object.
    Wrapped,
}

/// The binding a store writes into and how the target is named in a message.
struct StoreTarget<'a> {
    root: &'a str,
    described: String,
}

impl TypeChecker {
    /// Report `MER_TYP_081` when storing `rhs` into `lhs` makes the object
    /// `lhs` is rooted at hold a reference to itself.
    pub(crate) fn refuse_store_closing_a_cycle(
        &mut self,
        lhs: &LeftHandSideExpression,
        rhs: &Expression,
        span: Span,
        context: &Context,
    ) {
        let Some(target) = self.store_into_shared_object(lhs, context) else {
            return;
        };
        let Some(holding) = self.holding_of(rhs, target.root, context) else {
            return;
        };
        let (message, help) = cycle_diagnostic(holding, target.root, &target.described);
        self.report_error_with_help(DiagnosticCode::TypReferenceCycle, message, span, help);
    }

    /// The object a field or element store writes into, when the target is a
    /// place rooted at a binding that holds its object by reference.
    fn store_into_shared_object<'a>(
        &self,
        lhs: &'a LeftHandSideExpression,
        context: &Context,
    ) -> Option<StoreTarget<'a>> {
        let (base, described) = match lhs {
            LeftHandSideExpression::Identifier(_) => return None,
            LeftHandSideExpression::Member(e) => (e, format!("'{}'", place_text(e)?)),
            LeftHandSideExpression::Index(e) => {
                let ExpressionKind::Index(collection, _) = &e.node else {
                    return None;
                };
                (e, format!("an element of '{}'", place_text(collection)?))
            }
        };
        let root_expr = place_root(base)?;
        let ExpressionKind::Identifier(root, _) = &root_expr.node else {
            return None;
        };
        self.holds_by_reference(root_expr, context)
            .then_some(StoreTarget { root, described })
    }

    /// Whether the binding `expr` names holds its object by reference, so a
    /// copy of the binding is the same object: a class instance, or a value
    /// held through a trait. A built-in collection that copies on write is a
    /// value even though its type is a class: a store into one a closure also
    /// holds copies it first, so the closure keeps the collection as it was.
    /// A struct is a value the same way (`unshare` in MIR lowering), so it is
    /// not counted either.
    fn holds_by_reference(&self, expr: &Expression, context: &Context) -> bool {
        let Some(TypeKind::Custom(name, _)) = self.get_type(expr.id).map(|t| &t.kind) else {
            return false;
        };
        if BuiltinCollectionKind::from_name(name).is_some_and(|kind| cow_fn(kind).is_some()) {
            return false;
        }
        matches!(
            context
                .resolve_type_definition(name)
                .or_else(|| self.type_definitions().get(name)),
            Some(TypeDefinition::Class(_) | TypeDefinition::Trait(_))
        )
    }

    /// How `expr` holds the binding `root`, or `None` when it holds no
    /// reference to it that can be read off the expression.
    fn holding_of(&self, expr: &Expression, root: &str, context: &Context) -> Option<Holding> {
        match &expr.node {
            ExpressionKind::Identifier(name, _) => (name == root).then_some(Holding::Itself),
            ExpressionKind::Lambda(lambda) => collect_lambda_captures(lambda)
                .contains(root)
                .then_some(Holding::Captured),
            ExpressionKind::NamedArgument(_, inner) => self.holding_of(inner, root, context),
            ExpressionKind::Conditional(then_expr, _, else_expr, _) => {
                self.holding_of(then_expr, root, context).or_else(|| {
                    else_expr
                        .as_deref()
                        .and_then(|e| self.holding_of(e, root, context))
                })
            }
            ExpressionKind::Call(callee, args)
                if self.call_holds_its_arguments(callee, context) =>
            {
                self.wrapped_holding(args.iter(), root, context)
            }
            ExpressionKind::EnumValue(_, args)
            | ExpressionKind::List(args)
            | ExpressionKind::Set(args)
            | ExpressionKind::Tuple(args) => self.wrapped_holding(args.iter(), root, context),
            ExpressionKind::Array(elements, _) => {
                self.wrapped_holding(elements.iter(), root, context)
            }
            ExpressionKind::Map(entries) => {
                self.wrapped_holding(entries.iter().flat_map(|(k, v)| [k, v]), root, context)
            }
            ExpressionKind::Call(_, _)
            | ExpressionKind::Binary(_, _, _)
            | ExpressionKind::Logical(_, _, _)
            | ExpressionKind::Range(_, _, _)
            | ExpressionKind::Unary(_, _)
            | ExpressionKind::Index(_, _)
            | ExpressionKind::Member(_, _)
            | ExpressionKind::Assignment(_, _, _)
            | ExpressionKind::Block(_, _)
            | ExpressionKind::Cast(_, _)
            | ExpressionKind::Match(_, _)
            | ExpressionKind::Guard(_, _)
            | ExpressionKind::FormattedString(_)
            | ExpressionKind::Literal(_)
            | ExpressionKind::Super
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::StructMember(_, _) => None,
        }
    }

    /// How a value built from `parts` holds `root`: through a closure when one
    /// of them captures it, and as a wrapped reference otherwise.
    fn wrapped_holding<'e>(
        &self,
        parts: impl Iterator<Item = &'e Expression>,
        root: &str,
        context: &Context,
    ) -> Option<Holding> {
        parts
            .filter_map(|part| self.holding_of(part, root, context))
            .map(|holding| match holding {
                Holding::Captured => Holding::Captured,
                Holding::Itself | Holding::Wrapped => Holding::Wrapped,
            })
            .next()
    }

    /// Whether calling `callee` builds a value that keeps its arguments: a
    /// class or struct constructor, an enum variant, or a builtin variant
    /// (`Some`, `Ok`, `Err`) no binding in scope shadows.
    fn call_holds_its_arguments(&self, callee: &Expression, context: &Context) -> bool {
        if matches!(
            self.get_type(callee.id).map(|t| &t.kind),
            Some(TypeKind::Meta(_))
        ) {
            return true;
        }
        if self.is_variant_constructor(callee, context) {
            return true;
        }
        matches!(
            &callee.node,
            ExpressionKind::Identifier(name, _)
                if Self::is_builtin_variant_constructor(name) && context.resolve_info(name).is_none()
        )
    }
}

/// The message and help for a store that makes a cycle, given how the stored
/// value holds `root` and how the target is named.
fn cycle_diagnostic(holding: Holding, root: &str, described: &str) -> (String, String) {
    match holding {
        Holding::Captured => (
            format!(
                "this store makes a reference cycle: the closure captures '{root}' and is stored in {described}, and reference counting never frees a cycle"
            ),
            format!(
                "pass the object as a parameter instead of capturing it: give the function type a parameter for '{root}' and pass it at the call"
            ),
        ),
        Holding::Itself => (
            format!(
                "this store makes a reference cycle: {described} would hold '{root}' itself, and reference counting never frees a cycle"
            ),
            cycle_free_storage_help(),
        ),
        Holding::Wrapped => (
            format!(
                "this store makes a reference cycle: the value stored in {described} holds '{root}', and reference counting never frees a cycle"
            ),
            cycle_free_storage_help(),
        ),
    }
}

/// The help for a stored value that holds the object without a closure.
fn cycle_free_storage_help() -> String {
    "store a different object, or keep the link outside the object, in a collection its owner holds"
        .to_string()
}

/// The binding a place written as fields and elements of it starts from.
fn place_root(expr: &Expression) -> Option<&Expression> {
    if let ExpressionKind::Member(base, _) | ExpressionKind::Index(base, _) = &expr.node {
        return place_root(base);
    }
    matches!(expr.node, ExpressionKind::Identifier(_, _)).then_some(expr)
}

/// A place written as a binding and field names (`self.on_tick`), as text;
/// an element access within it is written `[…]`.
fn place_text(expr: &Expression) -> Option<String> {
    if let ExpressionKind::Member(base, field) = &expr.node {
        let ExpressionKind::Identifier(field, _) = &field.node else {
            return None;
        };
        return Some(format!("{}.{field}", place_text(base)?));
    }
    if let ExpressionKind::Index(base, _) = &expr.node {
        return Some(format!("{}[…]", place_text(base)?));
    }
    let ExpressionKind::Identifier(name, _) = &expr.node else {
        return None;
    };
    Some(name.clone())
}
