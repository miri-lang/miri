// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a container recognises two of its elements as the same one, and which
//! element types answer a container's questions through their own methods.
//!
//! The runtime sees an element only as the bytes of its slot. A set matching
//! its elements, or a map its keys, must apply the rule `==` applies to them:
//! a string by its content, a float by the number it holds, a struct or enum
//! through the equality synthesized from its `==`, a class, list or array
//! through its own `equals`. An [`ElementIdentity`] names that rule for one
//! element type, read by the type the element is, so a backend registers it
//! and decides nothing.

use super::TypeFacts;
use crate::ast::expression::Expression;
use crate::ast::types::{
    BuiltinCollectionKind, Type, TypeKind, ORDERING_TRAIT_NAME, SELF_TYPE_NAME, STRING_TYPE_NAME,
};
use crate::mir::dispatch::ELEMENT_METHOD_NAMES;
use crate::mir::option_layout::LayoutScope;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::type_checker::context::{MethodInfo, TypeDefinition};
use std::collections::HashSet;

/// A question a container asks two of its elements, answered by a method the
/// element type declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementQuestion {
    /// Which of the two sorts first: the type's `compare`, a negative, zero or
    /// positive pointer-sized integer.
    Compare,
    /// Whether the two are the same element: the type's own `equals`, a boolean.
    Equals,
}

impl ElementQuestion {
    /// Every question, in the order their thunks are emitted — the order of
    /// [`ELEMENT_METHOD_NAMES`].
    pub const ALL: [ElementQuestion; 2] = [ElementQuestion::Compare, ElementQuestion::Equals];

    /// The kind of thunk a container calls to ask this question.
    pub fn thunk_kind(self) -> ThunkKind {
        match self {
            ElementQuestion::Compare => ThunkKind::Compare,
            ElementQuestion::Equals => ThunkKind::Equals,
        }
    }

    /// The method this question calls, as [`ELEMENT_METHOD_NAMES`] spells it.
    pub fn method_name(self) -> &'static str {
        let [ordering, equals] = ELEMENT_METHOD_NAMES;
        match self {
            ElementQuestion::Compare => ordering,
            ElementQuestion::Equals => equals,
        }
    }
}

/// The width of a float element matched by the number it holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatWidth {
    F16,
    F32,
    F64,
}

/// The rule by which a set recognises two of its elements, or a map two of
/// its keys, as the same one.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementIdentity {
    /// No rule the runtime can apply: the element type names nothing
    /// concrete, or nothing `==` compares. Nothing is registered.
    Unregistered,
    /// The bytes of the value are the value.
    Bytes,
    /// The value points at a string, matched by its content.
    StringContent,
    /// The value is a float of this width, matched by the number it holds, so
    /// `-0.0` and `0.0` are one element and every NaN is one element.
    Float(FloatWidth),
    /// The value is held as its bytes, and its bytes are not its value: it is
    /// matched field by field, through the structural `equals` and `hash`
    /// thunks the backend emits for its layout.
    FieldWise,
    /// The value is matched through the thunks named here.
    Equals(Box<EqualityThunks>),
}

/// The thunks a set matches its elements, or a map its keys, through.
#[derive(Clone, Debug, PartialEq)]
pub struct EqualityThunks {
    /// The `equals` thunk two elements are compared through.
    pub equals: Symbol,
    /// The `hash` thunk consistent with `equals`, when there is one.
    pub hash: Option<Symbol>,
}

impl TypeFacts {
    /// How a set matches two elements, or a map two keys, of `kind`, in code
    /// leaving the parameters `open` unbound: the rule `==` applies to them.
    pub fn element_identity(&self, kind: &TypeKind, open: &HashSet<String>) -> ElementIdentity {
        let scope = LayoutScope::new(self.definitions(), open);
        if is_matched_field_wise(kind, scope) {
            return ElementIdentity::FieldWise;
        }
        let equals = crate::mir::dispatch::synthesized_equality_symbol(kind, self.definitions())
            .or_else(|| self.element_method_thunk(ElementQuestion::Equals, kind));
        if let Some(equals) = equals {
            let hash = crate::mir::dispatch::element_hash_symbol(kind, self.definitions());
            return ElementIdentity::Equals(Box::new(EqualityThunks { equals, hash }));
        }
        if let Some(width) = float_width(kind) {
            ElementIdentity::Float(width)
        } else if *kind == TypeKind::String {
            ElementIdentity::StringContent
        } else if is_matched_by_bytes(kind) {
            ElementIdentity::Bytes
        } else {
            ElementIdentity::Unregistered
        }
    }

    /// The thunk through which a container asks `question` of two elements of
    /// `kind`, or `None` when the element type does not answer it itself.
    ///
    /// A string, and a class, are ordered through their own `compare`; a
    /// class, list or array matches through its own `equals`. A recorded
    /// instantiation of a generic class is asked through the thunk of that
    /// instantiation, so the body compiled for the element's concrete type
    /// answers.
    pub fn element_method_thunk(
        &self,
        question: ElementQuestion,
        kind: &TypeKind,
    ) -> Option<Symbol> {
        let name = answering_type_name(question, kind)?;
        if !self.answers(name, question) {
            return None;
        }
        let recorded = self.element_method_instantiation(name, custom_type_args(kind));
        Some(Symbol::type_thunk(
            question.thunk_kind(),
            name,
            recorded.iter().flatten(),
        ))
    }

    /// Whether elements of `type_name` answer `question` themselves.
    ///
    /// Only a class does: a value type's bytes already answer it, and an enum
    /// may be stored as a bare discriminant the thunk's null guard would
    /// misread. A set or map of enums or structs is matched through the
    /// equality the pipeline synthesizes from their `==` instead, which calls
    /// an enum's own `equals` where it declares one.
    pub fn answers(&self, type_name: &str, question: ElementQuestion) -> bool {
        let Some(TypeDefinition::Class(_)) = self.definitions().get(type_name) else {
            return false;
        };
        match question {
            ElementQuestion::Compare => {
                self.implements(type_name, ORDERING_TRAIT_NAME)
                    && self
                        .inherited_method(type_name, question.method_name())
                        .is_some_and(|method| !method.is_abstract)
            }
            ElementQuestion::Equals => self
                .method_body(type_name, question.method_name())
                .is_some_and(|(declaring, method)| is_element_equality(declaring, method)),
        }
    }

    /// Whether a `type_name` element is copied through its own `clone`: the
    /// class, or a class it extends, implements `Cloneable`, directly or
    /// through a trait that extends it.
    pub fn is_cloneable(&self, type_name: &str) -> bool {
        self.implements(type_name, crate::ast::types::CLONEABLE_TRAIT_NAME)
    }

    /// Whether a clone thunk is emitted for `type_name`: it is cloneable and
    /// not an abstract class, whose `clone` may have no compiled body for a
    /// thunk to call.
    pub fn has_clone_thunk(&self, type_name: &str) -> bool {
        let is_abstract = matches!(
            self.definitions().get(type_name),
            Some(TypeDefinition::Class(class)) if class.is_abstract
        );
        !is_abstract && self.is_cloneable(type_name)
    }

    /// The instantiation of `class_name` whose element-method thunk is
    /// registered for its elements: the recorded one, when a method body was
    /// lowered for it, else `None` for the shared one.
    ///
    /// Thunks exist only for instantiations the pipeline monomorphizes, since
    /// each calls a method body lowered for its arguments. A body still
    /// written against a parameter — the shared copy of a generic function
    /// holding a `Set<Box<T>>` — records `Box<T>` as well, and naming its
    /// instantiation would reference a symbol nothing defines. A value
    /// argument — the size of a value generic — is read the way the registry
    /// records it, wrapped in a marker type, so an `Array<String, 2>` reaches
    /// the body lowered at `String` and `2`.
    pub fn element_method_instantiation(
        &self,
        class_name: &str,
        type_args: Option<&[Expression]>,
    ) -> Option<Vec<Type>> {
        self.definitions().get(class_name)?.generics()?;
        let concrete = type_args?
            .iter()
            .map(crate::mir::instantiation::instantiation_argument)
            .collect::<Option<Vec<Type>>>()?;
        if concrete.is_empty() {
            return None;
        }
        let monomorphized = concrete.iter().all(|arg| {
            crate::mir::instantiation::is_monomorphizable_type_argument(
                &arg.kind,
                self.definitions(),
            )
        });
        if !monomorphized {
            return None;
        }
        let want = Symbol::type_thunk(ThunkKind::Drop, class_name, &concrete);
        let recorded = self
            .generic_class_instantiations()
            .get(class_name)?
            .iter()
            .any(|tuple| Symbol::type_thunk(ThunkKind::Drop, class_name, tuple) == want);
        recorded.then_some(concrete)
    }
}

/// The type whose own method answers `question` for an element of `kind`, if
/// one could: a string or a class for ordering, a class, list or array for
/// equality. `Atomic<scalar>` is stored as its scalar and answers nothing.
fn answering_type_name(question: ElementQuestion, kind: &TypeKind) -> Option<&str> {
    if *kind == TypeKind::String {
        return (question == ElementQuestion::Compare).then_some(STRING_TYPE_NAME);
    }
    if let TypeKind::Custom(name, _) = kind {
        if name == crate::ast::types::ATOMIC_TYPE_NAME {
            return None;
        }
        if BuiltinCollectionKind::from_name(name).is_none() {
            return Some(name);
        }
    }
    let builtin = kind.as_builtin_collection()?;
    let compared_element_wise = matches!(
        builtin,
        BuiltinCollectionKind::List | BuiltinCollectionKind::Array
    );
    (question == ElementQuestion::Equals && compared_element_wise).then_some(builtin.name())
}

/// The type arguments a custom element type is written with.
fn custom_type_args(kind: &TypeKind) -> Option<&[Expression]> {
    if let TypeKind::Custom(_, args) = kind {
        args.as_deref()
    } else {
        None
    }
}

/// The width of a float element.
fn float_width(kind: &TypeKind) -> Option<FloatWidth> {
    if *kind == TypeKind::F16 {
        Some(FloatWidth::F16)
    } else if *kind == TypeKind::F32 {
        Some(FloatWidth::F32)
    } else if matches!(kind, TypeKind::Float | TypeKind::F64) {
        Some(FloatWidth::F64)
    } else {
        None
    }
}

/// True when `method` is an `equals` a container can call on two elements of
/// `type_name`: it has a body, takes one value of the class itself and answers
/// with a boolean.
///
/// The thunk hands the method two element pointers, so a method declared over
/// some other parameter type would read an element as a value it is not.
fn is_element_equality(type_name: &str, method: &MethodInfo) -> bool {
    let [(_, param_ty)] = method.params.as_slice() else {
        return false;
    };
    let takes_own_type = matches!(
        &param_ty.kind,
        TypeKind::Custom(name, _) if name == type_name || name == SELF_TYPE_NAME
    );
    !method.is_abstract
        && !method.is_static
        && takes_own_type
        && matches!(method.return_type.kind, TypeKind::Boolean)
}

/// Whether an element of `kind` is held as its bytes, those bytes are not its
/// value, and it has a structural thunk name: a tuple, an optional or a
/// function value laid out inline.
fn is_matched_field_wise(kind: &TypeKind, scope: LayoutScope<'_>) -> bool {
    let named = matches!(
        kind,
        TypeKind::Tuple(_) | TypeKind::Option(_) | TypeKind::Function(_)
    );
    named
        && crate::mir::type_facts::held_value_layout(kind, scope)
            .is_some_and(|layout| !layout.is_compared_by_bytes())
}

/// True when an element of `kind` is a scalar, whose bytes are its value.
fn is_matched_by_bytes(kind: &TypeKind) -> bool {
    matches!(
        kind,
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
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
    )
}
