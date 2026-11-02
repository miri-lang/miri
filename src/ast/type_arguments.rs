// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading the arguments a type is written with, without matching on the
//! expressions they are stored as.
//!
//! A type argument is stored as the expression it was written as, so that a
//! value generic's argument (the `4` of `Array<int, 4>`) fits the slot a type
//! argument (the `String` of `List<String>`) does. A stage past the type
//! checker reads an argument through [`argument_type`]: it is a type, or it is
//! a value, and the stage never asks which expression it was written as.
//!
//! TODO: the argument is still an AST expression underneath, so `TypeKind`
//! can hold any expression where a type is required, and only
//! [`unresolved_type_argument`] (run by `Body::validate`) catches one. Storing
//! each argument as a type or a value in `TypeKind` itself would make that
//! state unrepresentable; it touches the parser, the type checker, MIR and the
//! backends together.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::factory::{int_literal_expression, type_expr_non_null};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};

/// One argument a type is written with: a type, or a value generic's value.
pub type TypeArgument = Expression;

/// The arguments a type is written with, in order.
pub type TypeArguments = [TypeArgument];

/// The type `argument` names, or `None` when it names a value, such as an
/// array's size. A nullable mark written on the argument (`T?`) is not read
/// here; `mir::instantiation::type_argument` folds it into an optional.
pub fn argument_type(argument: &Expression) -> Option<&Type> {
    match &argument.node {
        ExpressionKind::Type(ty, _) => Some(ty),
        ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(..)
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

/// The type the argument at `position` of `arguments` names, or `None` when
/// there is no such argument or it names a value.
pub fn argument_type_at(arguments: &TypeArguments, position: usize) -> Option<&Type> {
    arguments.get(position).and_then(argument_type)
}

/// A type argument naming `ty`.
pub fn type_argument(ty: Type) -> TypeArgument {
    type_expr_non_null(ty)
}

/// Type arguments naming `types`, in order.
pub fn type_arguments(types: impl IntoIterator<Item = Type>) -> Vec<TypeArgument> {
    types.into_iter().map(type_argument).collect()
}

/// A value generic's argument holding `value`, such as an array's size.
pub fn value_argument(value: i128) -> TypeArgument {
    int_literal_expression(value)
}

/// The first argument within `kind` written where a type is required — a
/// collection's element, a map's key or value, a tuple's slot, a result's or
/// a future's payload — that names no type, searched through every type
/// argument `kind` holds. A value generic's own arguments may name values, so
/// only the built-in collections' positions are required to be types.
pub fn unresolved_type_argument(kind: &TypeKind) -> Option<&Expression> {
    let required: Vec<&Expression> = match kind {
        TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Array(element, _) => {
            vec![element]
        }
        TypeKind::Map(key, value) | TypeKind::Result(key, value) => vec![key, value],
        TypeKind::Future(payload) => vec![payload],
        TypeKind::Tuple(elements) => elements.iter().collect(),
        TypeKind::Custom(name, Some(args)) => match BuiltinCollectionKind::from_name(name) {
            Some(BuiltinCollectionKind::Map) => args.iter().take(2).collect(),
            Some(
                BuiltinCollectionKind::List
                | BuiltinCollectionKind::Set
                | BuiltinCollectionKind::Array,
            ) => args.iter().take(1).collect(),
            None => Vec::new(),
        },
        TypeKind::Option(inner) | TypeKind::Linear(inner) => {
            return unresolved_type_argument(&inner.kind)
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
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, None)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::OneOf(_) => Vec::new(),
    };
    if let Some(missing) = required.iter().find(|arg| argument_type(arg).is_none()) {
        return Some(missing);
    }
    let held: Vec<&Expression> = if let TypeKind::Custom(_, Some(args)) = kind {
        args.iter().collect()
    } else {
        required
    };
    held.into_iter()
        .filter_map(argument_type)
        .find_map(|ty| unresolved_type_argument(&ty.kind))
}
