// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The declared type a MIR place reaches, read from its body's declarations.
//!
//! A place names a local and a chain of projections. What it reaches is read
//! off the local's declared type one projection at a time — a field at the
//! instance's type arguments, an element of a collection, a closure capture —
//! so a question about it is asked of the type it actually holds, arguments
//! included, rather than of the root local's.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, TUPLE_TYPE_NAME};
use crate::mir::lowering::{field_type_in_instance, type_argument};
use crate::mir::types::MirType;
use crate::mir::{Body, Local, LocalDecl, Place, PlaceElem};
use std::collections::HashMap;

/// The declarations a body keeps that say what type a place reaches.
pub struct PlaceTypes<'a> {
    pub local_decls: &'a [LocalDecl],
    pub field_types: &'a HashMap<String, Vec<Type>>,
    pub class_type_params: &'a HashMap<String, Vec<String>>,
    pub closure_capture_types: &'a HashMap<Local, Vec<Type>>,
}

impl<'a> PlaceTypes<'a> {
    /// The place types of `body`.
    pub fn of(body: &'a Body) -> Self {
        Self {
            local_decls: &body.local_decls,
            field_types: &body.field_types,
            class_type_params: &body.class_type_params,
            closure_capture_types: &body.closure_capture_types,
        }
    }

    /// The declared type of what `place` reaches, carried along every
    /// projection, or `None` where the walk cannot resolve one: behind a
    /// dereference, or past an element or field whose type the body does not
    /// record.
    ///
    /// The walk decides which projection applies over [`MirType`], which
    /// stores collection element types resolved. Alongside it the declared
    /// type is carried through every projection, because `MirType::Custom`
    /// names a class without its type arguments.
    pub fn declared(&self, place: &Place) -> Option<Type> {
        let decl = self.local_decls.get(place.local.0)?;
        let mut current = decl.mir_ty.clone();
        let mut declared = decl.ty.clone();
        for elem in &place.projection {
            let (next, next_declared) = match elem {
                PlaceElem::Deref => None,
                PlaceElem::Index(_) => project_index(&current, &declared),
                PlaceElem::Field(i) => self.project_field(place, &current, &declared, *i),
            }?;
            current = next;
            declared = next_declared;
        }
        Some(declared)
    }

    /// The type a `Field(index)` projection yields from a value of type `current`,
    /// declared `declared`, paired with its declared type; `None` when the field
    /// cannot be resolved.
    fn project_field(
        &self,
        place: &Place,
        current: &MirType,
        declared: &Type,
        index: usize,
    ) -> Option<(MirType, Type)> {
        match current {
            // Option<T>.Field(0) → the inner type T
            MirType::Option(inner) if index == 0 => {
                let TypeKind::Option(payload) = &declared.kind else {
                    return None;
                };
                Some((*inner.clone(), (**payload).clone()))
            }
            // Tuple(T0, T1, …).Field(i) → Ti
            MirType::Tuple(elems) => {
                let TypeKind::Tuple(elements) = &declared.kind else {
                    return None;
                };
                Some((
                    elems.get(index)?.clone(),
                    element_type(elements.get(index)?)?,
                ))
            }
            // Custom(struct/class).Field(i) → the declared field type, read at the
            // instance's type arguments.
            MirType::Custom(name) | MirType::Vector(name) => {
                let declared_field = self.field_types.get(name.as_str())?.get(index)?;
                let field_ty = field_type_in_instance(
                    self.class_type_params,
                    name,
                    Some(declared),
                    declared_field,
                );
                Some((MirType::from_type_kind(&field_ty.kind), field_ty))
            }
            // Closure.Field(i) → the type of captured variable i.
            // Capture types are indexed by the root local because a closure
            // is always a single-level projection (never nested).
            MirType::Function => {
                let capture_ty = self.closure_capture_types.get(&place.local)?.get(index)?;
                Some((
                    MirType::from_type_kind(&capture_ty.kind),
                    capture_ty.clone(),
                ))
            }
            MirType::Option(_)
            | MirType::Int
            | MirType::I8
            | MirType::I16
            | MirType::I32
            | MirType::I64
            | MirType::I128
            | MirType::U8
            | MirType::U16
            | MirType::U32
            | MirType::U64
            | MirType::U128
            | MirType::Float
            | MirType::F16
            | MirType::F32
            | MirType::F64
            | MirType::Boolean
            | MirType::Void
            | MirType::Identifier
            | MirType::RawPtr
            | MirType::Error
            | MirType::String
            | MirType::List(_)
            | MirType::Array(_)
            | MirType::Map(_, _)
            | MirType::Set(_)
            | MirType::Result(_, _)
            | MirType::Future(_)
            | MirType::Generic
            | MirType::Unknown => None,
        }
    }
}

/// The element an `Index` projection yields from a collection of type
/// `current`, declared `declared`; `None` when it is no collection, or when
/// its element is a vector, which lives inline in the collection's buffer:
/// indexing yields an interior address rather than a pointer to an
/// allocation of its own, and retaining or releasing it would reach into the
/// buffer's bytes.
fn project_index(current: &MirType, declared: &Type) -> Option<(MirType, Type)> {
    if let Some(element) = tuple_element(declared) {
        return Some((MirType::from_type_kind(&element.kind), element));
    }
    let (MirType::Array(element)
    | MirType::List(element)
    | MirType::Set(element)
    | MirType::Map(_, element)) = current
    else {
        return None;
    };
    if element.is_inline_vector() {
        return None;
    }
    Some((*element.clone(), indexed_element_type(declared)?))
}

/// The element an `Index` projection yields from a tuple picked by a value:
/// a tuple, homogeneous whenever it is indexed so, yields its first element's
/// type, and the tuple class its methods take one as yields its argument.
fn tuple_element(declared: &Type) -> Option<Type> {
    if let TypeKind::Tuple(elements) = &declared.kind {
        return element_type(elements.first()?);
    }
    let TypeKind::Custom(name, Some(args)) = &declared.kind else {
        return None;
    };
    if name != TUPLE_TYPE_NAME {
        return None;
    }
    element_type(args.first()?)
}

/// The declared type an `Index` projection yields from a collection.
///
/// The walk's `MirType` keeps a collection's element resolved, but only the
/// declared spelling still carries a class element's type arguments — a
/// `List<Tagged<String>>` indexes to `Tagged<String>`, where the `MirType` is
/// `Custom("Tagged")` and names the class alone. Carrying the argument through
/// is what lets a field declared at the class parameter be read at the type it
/// actually holds. Indexing a sequence or a set yields its element; indexing a
/// map yields its value.
fn indexed_element_type(collection: &Type) -> Option<Type> {
    element_type(collection_element(&collection.kind)?)
}

/// The element expression of a collection type, or its value for a map;
/// `None` for every other kind.
fn collection_element(kind: &TypeKind) -> Option<&Expression> {
    match kind {
        TypeKind::Custom(name, Some(args)) => match BuiltinCollectionKind::from_name(name)? {
            BuiltinCollectionKind::Array
            | BuiltinCollectionKind::List
            | BuiltinCollectionKind::Set => args.first(),
            BuiltinCollectionKind::Map => args.get(1),
        },
        TypeKind::List(element) | TypeKind::Array(element, _) | TypeKind::Set(element) => {
            Some(element)
        }
        TypeKind::Map(_, value) => Some(value),
        TypeKind::Custom(_, None)
        | TypeKind::Int
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
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::OneOf(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Option(_)
        | TypeKind::Future(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Function(_)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_) => None,
    }
}

/// The type an element expression of a collection or tuple type denotes: a
/// written type, or a bare name, which names a type as written.
fn element_type(element: &Expression) -> Option<Type> {
    if let ExpressionKind::Identifier(name, _) = &element.node {
        return Some(Type::new(
            TypeKind::Custom(name.clone(), None),
            element.span,
        ));
    }
    type_argument(element)
}
