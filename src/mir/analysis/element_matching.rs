// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What code the runtime runs when it matches or orders the elements of a
//! container.
//!
//! A set, a map, or a sort tells two elements apart by the rule the compiler
//! registered on the container when it was built: a string by its content, a
//! number or a reference by its bytes, and anything else through a callback.
//! A struct's or an enum's callback is the equality or hash the pipeline
//! lowers from its `==` (`mir::dispatch::synthesized_equality_symbol`,
//! `element_hash_symbol`), a body of the program whose own effects say what
//! it does. A class with an `equals`, `hash` or `compare` of its own — or a
//! subclass that has one — runs that method, and an element whose type is a
//! trait, a type parameter or a type that cannot be read may run anything.
//!
//! The rule is registered where the container is built, so a body shared by
//! every element type cannot tell which one it is: an element type naming a
//! parameter the body leaves open is reported as such, for the call handing
//! that body a container to answer.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{
    BuiltinCollectionKind, Type, TypeKind, EQUALS_METHOD_NAME, HASH_METHOD_NAME,
    ORDERING_METHOD_NAME,
};
use crate::mir::dispatch::{element_hash_symbol, synthesized_equality_symbol};
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::Symbol;
use crate::mir::type_facts::TypeDefinition;
use crate::type_checker::context::{class_ancestry, class_method_declaration};
use std::collections::HashMap;

/// How deep into an element type the walk follows its parts before taking
/// the rest to run anything.
const MAX_DEPTH: usize = 16;

/// What the runtime asks of the elements it is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// Whether two are the same element, and where one is placed: a set, a
    /// map's keys.
    Matches,
    /// Which of two sorts first.
    Orders,
}

/// The code matching or ordering one element type runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Matching {
    /// The element type runs code whose effects the program cannot read off
    /// a body: a method of a class, a trait or a type parameter's own.
    pub runs_unknown_code: bool,
    /// The element type names a parameter the asking body leaves open.
    pub names_open_parameter: bool,
    /// The bodies of the program matching the element type calls.
    pub callbacks: Vec<Symbol>,
}

impl Matching {
    /// Whether matching runs no code of the program at all.
    pub fn is_by_value(&self) -> bool {
        !self.runs_unknown_code && !self.names_open_parameter && self.callbacks.is_empty()
    }

    /// `self` together with `other`.
    pub fn merge(&mut self, other: Matching) {
        self.runs_unknown_code |= other.runs_unknown_code;
        self.names_open_parameter |= other.names_open_parameter;
        self.callbacks.extend(other.callbacks);
    }
}

/// The type of the elements, or the keys, a container of type `container`
/// matches or orders; `None` for a type that is no built-in collection, or
/// whose element type cannot be read.
///
/// A collection spelled without its arguments — the receiver of a body
/// shared by every instantiation of the collection's class — holds elements
/// of the class's own first type parameter, which `definitions` names.
pub fn matched_element(
    container: &Type,
    definitions: &HashMap<String, TypeDefinition>,
) -> Option<Type> {
    if let TypeKind::List(element)
    | TypeKind::Array(element, _)
    | TypeKind::Set(element)
    | TypeKind::Map(element, _) = &container.kind
    {
        return written_type(element).cloned();
    }
    let TypeKind::Custom(name, args) = &container.kind else {
        return None;
    };
    BuiltinCollectionKind::from_name(name)?;
    match args {
        Some(args) => args.first().and_then(written_type).cloned(),
        None => {
            let parameter = definitions.get(name)?.generics()?.first()?;
            let kind = TypeKind::Generic(parameter.name.clone(), None, parameter.kind);
            Some(Type::new(kind, container.span))
        }
    }
}

/// The code answering `question` about elements of type `element` runs, as
/// a body whose parameters `types` reads asks it.
pub fn element_matching(element: &Type, types: &PlaceTypes, question: Question) -> Matching {
    if types.names_open_parameter(&element.kind) {
        return Matching {
            names_open_parameter: true,
            ..Matching::default()
        };
    }
    let mut matching = Matching::default();
    Walk {
        definitions: types.definitions,
        question,
        found: &mut matching,
    }
    .visit(&element.kind, 0);
    matching
}

/// One question about one element type, following its parts.
struct Walk<'d, 'm> {
    definitions: &'d HashMap<String, TypeDefinition>,
    question: Question,
    found: &'m mut Matching,
}

impl Walk<'_, '_> {
    fn visit(&mut self, kind: &TypeKind, depth: usize) {
        if depth > MAX_DEPTH {
            self.found.runs_unknown_code = true;
            return;
        }
        match kind {
            TypeKind::Option(inner) | TypeKind::Linear(inner) => self.visit(&inner.kind, depth + 1),
            TypeKind::Tuple(parts) => {
                for part in parts {
                    self.visit_written(part, depth + 1);
                }
            }
            TypeKind::Custom(name, _) => self.visit_named(kind, name),
            // A list or an array is matched element by element through the
            // `equals` its class declares.
            TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Generic(..)
            | TypeKind::Function(_)
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::OneOf(_) => self.found.runs_unknown_code = true,
            // A number, a string and a reference are matched by the runtime
            // alone: by bytes, by content, by address.
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
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Meta(_)
            | TypeKind::Void
            | TypeKind::Error => {}
        }
    }

    fn visit_written(&mut self, expression: &Expression, depth: usize) {
        match written_type(expression) {
            Some(ty) => self.visit(&ty.kind, depth),
            None => self.found.runs_unknown_code = true,
        }
    }

    /// The rule for the named type `kind`, called `name`.
    fn visit_named(&mut self, kind: &TypeKind, name: &str) {
        match BuiltinCollectionKind::from_name(name) {
            // A set or a map element is matched by its reference.
            Some(BuiltinCollectionKind::Set | BuiltinCollectionKind::Map) => return,
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array) => {
                self.found.runs_unknown_code = true;
                return;
            }
            None => {}
        }
        match self.definitions.get(name) {
            Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_)) => self.visit_data(kind),
            Some(TypeDefinition::Class(_)) => {
                self.found.runs_unknown_code |= self.has_own_identity(name);
            }
            Some(
                TypeDefinition::Trait(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_),
            )
            | None => self.found.runs_unknown_code = true,
        }
    }

    /// A struct or an enum, matched through the equality and hash lowered
    /// for it; ordered through code no body of the program shows.
    fn visit_data(&mut self, kind: &TypeKind) {
        let equality = synthesized_equality_symbol(kind, self.definitions);
        match (self.question, equality) {
            (Question::Matches, Some(equality)) => {
                self.found.callbacks.push(equality);
                self.found
                    .callbacks
                    .extend(element_hash_symbol(kind, self.definitions));
            }
            (Question::Matches, None) | (Question::Orders, _) => {
                self.found.runs_unknown_code = true;
            }
        }
    }

    /// Whether the class `name`, a class it extends or a class extending it
    /// declares the method answering the question.
    fn has_own_identity(&self, name: &str) -> bool {
        let methods: &[&str] = match self.question {
            Question::Matches => &[EQUALS_METHOD_NAME, HASH_METHOD_NAME],
            Question::Orders => &[ORDERING_METHOD_NAME],
        };
        let definitions = self.definitions;
        definitions
            .iter()
            .filter(|(class, definition)| {
                matches!(definition, TypeDefinition::Class(_))
                    && class_ancestry(class, definitions).any(|(ancestor, _)| ancestor == name)
            })
            .any(|(class, _)| {
                methods
                    .iter()
                    .any(|method| class_method_declaration(class, method, definitions).is_some())
            })
    }
}

/// The type an expression spells, when it spells one.
fn written_type(expression: &Expression) -> Option<&Type> {
    let ExpressionKind::Type(ty, _) = &expression.node else {
        return None;
    };
    Some(ty)
}
