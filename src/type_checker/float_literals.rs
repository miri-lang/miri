// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The width a float literal takes when nothing constrains it.
//!
//! A decimal literal is written without a width — Miri has neither literal
//! suffixes nor a type annotation on every binding — so the width is a typing
//! decision. Where a context supplies one (a declared binding, a parameter, a
//! return type, a buffer element), the literal takes it. Where nothing does,
//! the literal takes the widest float the **target** can represent, which is
//! what this module decides.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

/// Whether a type is one of the float widths a literal can be narrowed to.
pub(crate) fn is_float_width(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
    )
}

/// The widest float a CPU target can represent.
///
/// This does not follow the CPU's pointer width: a 32-bit (or narrower) CPU
/// still evaluates `f64`, in hardware or through its ABI's soft-float
/// routines, so it is register width rather than float width that a narrow CPU
/// constrains. `Float` is the same width-less spelling `Int` has on the
/// integer side, so a literal typed with it still narrows into any float a
/// context declares.
pub(crate) fn cpu_float_width() -> TypeKind {
    TypeKind::Float
}

/// The widest float a GPU target can represent.
///
/// WGSL has an `f64` in neither of the profiles this backend emits — no
/// `enable` directive covers it — so a kernel cannot hold one, and neither can
/// a buffer a kernel reads. An unconstrained literal bound for the GPU must
/// therefore land at `f32` rather than emit a width the shader cannot compile.
pub(crate) fn gpu_float_width() -> TypeKind {
    TypeKind::F32
}

/// The width an unconstrained float literal takes on the target currently
/// being checked.
///
/// Defaulting to the target's widest float keeps a literal from silently
/// losing precision it was written with: a value rounds only where a context
/// explicitly asks for a narrower width.
///
/// Code targets the GPU either by position — inside a `gpu fn` or a `forall`
/// body — or by residency: the initializer of a `gpu let` / `gpu var` is
/// written in host scope but its values are uploaded to a device buffer, so it
/// is bound by the device's widths, not the host's.
pub(crate) fn default_float_literal_width(context: &Context) -> TypeKind {
    if targets_gpu(context) {
        gpu_float_width()
    } else {
        cpu_float_width()
    }
}

/// Whether the code being checked runs on the GPU, either by position — inside
/// a `gpu fn` or a `forall` body — or by residency: the initializer of a
/// `gpu let` / `gpu var` is written in host scope but its values are uploaded
/// to a device buffer, so it is bound by the device's widths, not the host's.
fn targets_gpu(context: &Context) -> bool {
    context.in_gpu_function || context.in_gpu_resident_initializer
}

/// The width a literal actually takes when a context declares `declared`.
///
/// A declared width is a request, not a guarantee: the target still has to be
/// able to represent it. A host signature reused inside a kernel is the case
/// that makes the difference visible — a stdlib `clamp(x float, ...)` declares
/// f64 parameters, but a literal passed to it from GPU code has to stay f32,
/// because that call is emitted as WGSL where no f64 exists. Clamping here is
/// what keeps a declared width from pulling a literal past what its target can
/// hold.
fn width_for_target(declared: &TypeKind, context: &Context) -> TypeKind {
    if targets_gpu(context) && matches!(declared, TypeKind::Float | TypeKind::F64) {
        gpu_float_width()
    } else {
        declared.clone()
    }
}

impl TypeChecker {
    /// Retypes the float literals written inside `expr` to the width `expected`
    /// declares, returning the type `expr` has once they are narrowed.
    ///
    /// Only literals move. A value that already has a width — a binding, a call
    /// result, an element read out of a collection — keeps it, so narrowing can
    /// never silently round away precision a program computed; it only decides
    /// the width of a number the source spelled out and left open.
    ///
    /// Narrowing rewrites the recorded type rather than the AST, which is what
    /// makes MIR lower the constant at the declared width. The width taken is
    /// the declared one clamped to the target (see [`width_for_target`]), so a
    /// host signature called from GPU code cannot pull a literal to a width the
    /// device has no type for.
    ///
    /// A collection literal narrows through its elements and reports the
    /// element width it took, while the rest of its type — an array's size
    /// above all — stays as inference found it, so narrowing can widen what a
    /// context accepts but never hide a shape mismatch.
    pub(crate) fn narrow_float_literals(
        &mut self,
        expr: &Expression,
        expected: &Type,
        inferred: &Type,
        context: &Context,
    ) -> Option<Type> {
        match &expr.node {
            ExpressionKind::Literal(Literal::Float(_))
                if is_float_width(&expected.kind) && is_float_width(&inferred.kind) =>
            {
                let width = Type::new(width_for_target(&expected.kind, context), expected.span);
                self.record_narrowed_type(expr, &width);
                Some(width)
            }
            ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand) => {
                let narrowed = self.narrow_float_literals(operand, expected, inferred, context)?;
                self.record_narrowed_type(expr, &narrowed);
                Some(narrowed)
            }
            // A collection constructor wrapping a literal (`List([1.5, 2.5])`)
            // narrows through to that literal: the constructor only chooses the
            // collection, the elements are still written in the source.
            ExpressionKind::Call(_, args)
                if args.len() == 1
                    && matches!(
                        args[0].node,
                        ExpressionKind::List(_) | ExpressionKind::Array(_, _)
                    )
                    && self.sequence_element_type(expected).is_some()
                    && self.sequence_element_type(inferred).is_some() =>
            {
                let literal_inferred = self.type_table.types.get(&args[0].id)?.clone();
                let narrowed_literal =
                    self.narrow_float_literals(&args[0], expected, &literal_inferred, context)?;
                let element = self.sequence_element_type(&narrowed_literal)?;
                let narrowed = self.with_element_type(inferred, &element)?;
                self.record_narrowed_type(expr, &narrowed);
                Some(narrowed)
            }
            ExpressionKind::List(elements) | ExpressionKind::Array(elements, _) => {
                let expected_element = self.sequence_element_type(expected)?;
                let inferred_element = self.sequence_element_type(inferred)?;
                let mut narrowed_element = None;
                for element in elements {
                    narrowed_element = self.narrow_float_literals(
                        element,
                        &expected_element,
                        &inferred_element,
                        context,
                    );
                    narrowed_element.as_ref()?;
                }
                let narrowed = self.with_element_type(inferred, &narrowed_element?)?;
                self.record_narrowed_type(expr, &narrowed);
                Some(narrowed)
            }
            _ => None,
        }
    }

    /// Records `ty` as the type of `expr`, replacing what inference recorded.
    fn record_narrowed_type(&mut self, expr: &Expression, ty: &Type) {
        self.type_table
            .types
            .insert(expr.id, Type::new(ty.kind.clone(), expr.span));
    }

    /// The element type of a built-in sequence type (`List`, `Array`, `Set`),
    /// or `None` for anything a collection literal cannot narrow into. A `Map`
    /// literal is not a sequence of elements, so it narrows nowhere here.
    ///
    /// Both spellings of a collection type are accepted: the canonical
    /// `TypeKind::List`/`Array`/`Set` variants a field or parameter declaration
    /// carries, and the normalized `Custom("List", [..])` form inference
    /// produces.
    fn sequence_element_type(&self, collection: &Type) -> Option<Type> {
        match &collection.kind {
            TypeKind::List(inner) | TypeKind::Set(inner) | TypeKind::Array(inner, _) => {
                self.extract_type_from_expression(inner).ok()
            }
            TypeKind::Custom(name, Some(args)) => {
                match BuiltinCollectionKind::from_name(name.as_str())? {
                    BuiltinCollectionKind::List
                    | BuiltinCollectionKind::Array
                    | BuiltinCollectionKind::Set => {
                        self.extract_type_from_expression(args.first()?).ok()
                    }
                    BuiltinCollectionKind::Map => None,
                }
            }
            _ => None,
        }
    }

    /// `collection` with its element type replaced, every other part of the
    /// type (an array's size) left as it was.
    fn with_element_type(&mut self, collection: &Type, element: &Type) -> Option<Type> {
        let element_expr = self.create_type_expression(element.clone());
        let kind = match &collection.kind {
            TypeKind::List(_) => TypeKind::List(Box::new(element_expr)),
            TypeKind::Set(_) => TypeKind::Set(Box::new(element_expr)),
            TypeKind::Array(_, size) => {
                TypeKind::Array(Box::new(element_expr), Box::new((**size).clone()))
            }
            TypeKind::Custom(name, Some(args)) => {
                let mut narrowed_args = args.clone();
                *narrowed_args.first_mut()? = element_expr;
                TypeKind::Custom(name.clone(), Some(narrowed_args))
            }
            _ => return None,
        };
        Some(Type::new(kind, collection.span))
    }
}
