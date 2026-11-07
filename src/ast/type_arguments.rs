// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The arguments a type is written with: each a type, or a value generic's
//! value.
//!
//! An argument is a type (the `String` of `List<String>`) or a value (the `4`
//! of `Array<int, 4>`), and nothing else: no expression of any other kind can
//! stand where a type argument is written. A value is an integer, a name — a
//! constant's, or a value parameter's in a generic body — or arithmetic over
//! them (`Array<f32, W * W>`); substitution and the type checker's folding
//! leave only integers for the stages past it, which read them through
//! [`argument_value`].
//!
//! An argument is a node like any other in the tree: it carries the span it
//! was written at and the id the type checker keys what it resolved a written
//! name to by. Two arguments are equal when they say the same thing, wherever
//! they were written.

use std::fmt;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::factory::{self, next_id};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::node::IdNode;
use crate::ast::operator::{BinaryOp, UnaryOp};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::syntax::Span;

/// One argument a type is written with.
pub type TypeArgument = IdNode<TypeArgKind>;

/// The arguments a type is written with, in order.
pub type TypeArguments = [TypeArgument];

/// What one type argument says.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TypeArgKind {
    /// A type, and whether it was written with a nullable mark (`T?`). The
    /// mark is read as an optional of the type; see [`argument_type_marked`].
    Type(Box<Type>, bool),
    /// A value generic's value.
    Value(ValueArg),
}

/// A value generic's value, as written.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueArg {
    Int(i128),
    /// A constant, or a value parameter of the generic declaration the
    /// argument is written in.
    Name(String),
    Binary(ArithOp, Box<ValueArg>, Box<ValueArg>),
    /// An integer written past the signed 128-bit range, kept as its sign and
    /// magnitude so the type checker refuses it where it is written. It has
    /// no value, so no stage past the type checker holds one.
    OutOfRange {
        negative: bool,
        magnitude: u128,
    },
}

/// The arithmetic a value argument may be written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArithOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
}

impl ArithOp {
    /// `l op r`, or `None` when it overflows or divides by zero.
    pub fn apply(self, l: i128, r: i128) -> Option<i128> {
        match self {
            ArithOp::Add => l.checked_add(r),
            ArithOp::Sub => l.checked_sub(r),
            ArithOp::Mul => l.checked_mul(r),
            ArithOp::Div => l.checked_div(r),
            ArithOp::Mod => l.checked_rem(r),
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            ArithOp::Add => "+",
            ArithOp::Sub => "-",
            ArithOp::Mul => "*",
            ArithOp::Div => "/",
            ArithOp::Mod => "%",
        }
    }

    fn binary(self) -> BinaryOp {
        match self {
            ArithOp::Add => BinaryOp::Add,
            ArithOp::Sub => BinaryOp::Sub,
            ArithOp::Mul => BinaryOp::Mul,
            ArithOp::Div => BinaryOp::Div,
            ArithOp::Mod => BinaryOp::Mod,
        }
    }

    fn of(op: &BinaryOp) -> Option<Self> {
        Some(match op {
            BinaryOp::Add => ArithOp::Add,
            BinaryOp::Sub => ArithOp::Sub,
            BinaryOp::Mul => ArithOp::Mul,
            BinaryOp::Div => ArithOp::Div,
            BinaryOp::Mod => ArithOp::Mod,
            BinaryOp::BitwiseOr
            | BinaryOp::BitwiseAnd
            | BinaryOp::BitwiseXor
            | BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::LessThan
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual
            | BinaryOp::Not
            | BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Range
            | BinaryOp::In
            | BinaryOp::NullCoalesce => return None,
        })
    }
}

impl ValueArg {
    /// The value, folding its arithmetic, with each name read through
    /// `name`; `None` when a name has no value or the arithmetic overflows
    /// or divides by zero.
    pub fn fold(&self, name: &mut impl FnMut(&str) -> Option<i128>) -> Option<i128> {
        match self {
            ValueArg::Int(value) => Some(*value),
            ValueArg::OutOfRange { .. } => None,
            ValueArg::Name(n) => name(n),
            ValueArg::Binary(op, l, r) => {
                let l = l.fold(name)?;
                let r = r.fold(name)?;
                op.apply(l, r)
            }
        }
    }

    /// The value with each name `name` maps replaced by what it maps it to.
    pub fn substituted(&self, name: &impl Fn(&str) -> Option<ValueArg>) -> ValueArg {
        match self {
            ValueArg::Int(_) | ValueArg::OutOfRange { .. } => self.clone(),
            ValueArg::Name(n) => name(n).unwrap_or_else(|| self.clone()),
            ValueArg::Binary(op, l, r) => ValueArg::Binary(
                *op,
                Box::new(l.substituted(name)),
                Box::new(r.substituted(name)),
            ),
        }
    }
}

impl ValueArg {
    /// The expression this value is written as, at `span`: what renders it
    /// back to source the way any other expression is.
    pub fn expression(&self, span: Span) -> Expression {
        match self {
            ValueArg::Int(value) if *value < 0 && *value != i128::MIN => factory::unary_with_span(
                UnaryOp::Negate,
                factory::int_literal_expression(-*value),
                span,
            ),
            ValueArg::Int(value) => factory::int_literal_expression(*value),
            ValueArg::OutOfRange {
                negative,
                magnitude,
            } => {
                let literal = factory::literal_with_span(
                    Literal::Integer(IntegerLiteral::U128(*magnitude)),
                    span,
                );
                if *negative {
                    factory::unary_with_span(UnaryOp::Negate, literal, span)
                } else {
                    literal
                }
            }
            ValueArg::Name(name) => factory::identifier_with_span(name, span),
            ValueArg::Binary(op, l, r) => {
                factory::binary_with_span(l.expression(span), op.binary(), r.expression(span), span)
            }
        }
    }
}

impl fmt::Display for ValueArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueArg::Int(value) => write!(f, "{value}"),
            ValueArg::OutOfRange {
                negative,
                magnitude,
            } => {
                write!(f, "{}{magnitude}", if *negative { "-" } else { "" })
            }
            ValueArg::Name(name) => f.write_str(name),
            ValueArg::Binary(op, l, r) => {
                let operand = |f: &mut fmt::Formatter<'_>, v: &ValueArg| match v {
                    ValueArg::Binary(..) => write!(f, "({v})"),
                    ValueArg::Int(_) | ValueArg::Name(_) | ValueArg::OutOfRange { .. } => {
                        write!(f, "{v}")
                    }
                };
                operand(f, l)?;
                write!(f, " {} ", op.symbol())?;
                operand(f, r)
            }
        }
    }
}

impl fmt::Display for TypeArgKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeArgKind::Type(ty, _) => write!(f, "{ty}"),
            TypeArgKind::Value(value) => write!(f, "{value}"),
        }
    }
}

/// The type `argument` names, or `None` when it names a value, such as an
/// array's size. A nullable mark written on the argument (`T?`) is not read
/// here; [`argument_type_marked`] reads it.
pub fn argument_type(argument: &TypeArgument) -> Option<&Type> {
    match &argument.node {
        TypeArgKind::Type(ty, _) => Some(ty),
        TypeArgKind::Value(_) => None,
    }
}

/// The type `argument` names, with a nullable mark written on it read as an
/// optional of that type; `None` when it names a value.
pub fn argument_type_marked(argument: &TypeArgument) -> Option<Type> {
    match &argument.node {
        TypeArgKind::Type(ty, false) => Some((**ty).clone()),
        TypeArgKind::Type(ty, true) => Some(Type::new(TypeKind::Option(ty.clone()), ty.span)),
        TypeArgKind::Value(_) => None,
    }
}

/// The type the argument at `position` of `arguments` names, or `None` when
/// there is no such argument or it names a value.
pub fn argument_type_at(arguments: &TypeArguments, position: usize) -> Option<&Type> {
    arguments.get(position).and_then(argument_type)
}

/// The value `argument` holds once folded to an integer; `None` when it
/// names a type, or a value not yet folded.
pub fn argument_value(argument: &TypeArgument) -> Option<i128> {
    match &argument.node {
        TypeArgKind::Value(ValueArg::Int(value)) => Some(*value),
        TypeArgKind::Value(
            ValueArg::Name(_) | ValueArg::Binary(..) | ValueArg::OutOfRange { .. },
        )
        | TypeArgKind::Type(..) => None,
    }
}

/// The integer `argument` writes past the signed 128-bit range, as written,
/// when it writes one anywhere in its arithmetic.
pub fn out_of_range_value(argument: &TypeArgument) -> Option<&ValueArg> {
    fn find(value: &ValueArg) -> Option<&ValueArg> {
        match value {
            ValueArg::OutOfRange { .. } => Some(value),
            ValueArg::Binary(_, left, right) => find(left).or_else(|| find(right)),
            ValueArg::Int(_) | ValueArg::Name(_) => None,
        }
    }
    match &argument.node {
        TypeArgKind::Value(value) => find(value),
        TypeArgKind::Type(..) => None,
    }
}

/// A type argument naming `ty`, written nowhere in the source.
pub fn type_argument(ty: Type) -> TypeArgument {
    IdNode::new(
        next_id(),
        TypeArgKind::Type(Box::new(ty), false),
        Span::new(0, 0),
    )
}

/// A type argument naming `ty` written at `span`, with a nullable mark when
/// `nullable`.
pub fn marked_type_argument(ty: Type, nullable: bool, span: Span) -> TypeArgument {
    IdNode::new(next_id(), TypeArgKind::Type(Box::new(ty), nullable), span)
}

/// Type arguments naming `types`, in order.
pub fn type_arguments(types: impl IntoIterator<Item = Type>) -> Vec<TypeArgument> {
    types.into_iter().map(type_argument).collect()
}

/// A value generic's argument holding `value`, such as an array's size.
pub fn value_argument(value: i128) -> TypeArgument {
    IdNode::new(
        next_id(),
        TypeArgKind::Value(ValueArg::Int(value)),
        Span::new(0, 0),
    )
}

/// The first argument within `kind` that holds a value where a type is
/// required: a collection's element, a map's key or value, a tuple's slot, or
/// a result's or a future's payload. A value generic's own arguments may name
/// values, so only the built-in collections' positions are required to be
/// types.
///
/// This search is how the rule is enforced, not a stopgap. After parsing, a
/// built-in collection is spelled as a class, `Custom("List", [T])`, and a
/// class's argument list must hold types and values side by side
/// (`Buf<T, Size>`), so the representation cannot make a value in a
/// collection's element slot unbuildable. `Body::validate` refuses what this
/// finds.
pub fn unresolved_type_argument(kind: &TypeKind) -> Option<&TypeArgument> {
    let required: Vec<&TypeArgument> = match kind {
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
        | TypeKind::Generic(_, _)
        | TypeKind::Custom(_, None)
        | TypeKind::Meta(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::OneOf(_) => Vec::new(),
    };
    if let Some(missing) = required.iter().find(|arg| argument_type(arg).is_none()) {
        return Some(missing);
    }
    let held: Vec<&TypeArgument> = if let TypeKind::Custom(_, Some(args)) = kind {
        args.iter().collect()
    } else {
        required
    };
    held.into_iter()
        .filter_map(argument_type)
        .find_map(|ty| unresolved_type_argument(&ty.kind))
}

/// `argument` saying `kind` instead, written at the same place.
pub fn rewritten(argument: &TypeArgument, kind: TypeArgKind) -> TypeArgument {
    IdNode::new(argument.id, kind, argument.span)
}

/// The type argument `expr` writes: a type expression's type, or the value an
/// integer, a name or arithmetic over them spells. Any other expression is
/// handed back for the parser to refuse.
pub fn written_argument(expr: Expression) -> Result<TypeArgument, Expression> {
    let kind = match &expr.node {
        ExpressionKind::Type(ty, nullable) => TypeArgKind::Type(ty.clone(), *nullable),
        _ => match written_value(&expr) {
            Some(value) => TypeArgKind::Value(value),
            None => return Err(expr),
        },
    };
    Ok(IdNode::new(expr.id, kind, expr.span))
}

/// The value `expr` spells, when it is an integer, a name or arithmetic over
/// them. A negated integer is folded into the integer, which is what lets
/// `i128::MIN` be written.
fn written_value(expr: &Expression) -> Option<ValueArg> {
    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(value)) => Some(match value.above_i128() {
            Some(magnitude) => ValueArg::OutOfRange {
                negative: false,
                magnitude,
            },
            None => ValueArg::Int(value.to_i128()),
        }),
        ExpressionKind::Identifier(name, None) => Some(ValueArg::Name(name.clone())),
        ExpressionKind::Type(ty, false) => match &ty.kind {
            TypeKind::Custom(name, None) if !name.contains("::") => {
                Some(ValueArg::Name(name.clone()))
            }
            _ => None,
        },
        ExpressionKind::Unary(UnaryOp::Negate, inner) => match &inner.node {
            ExpressionKind::Literal(Literal::Integer(value)) => Some(match value.negated() {
                Some(negated) => ValueArg::Int(negated),
                None => ValueArg::OutOfRange {
                    negative: true,
                    magnitude: value.above_i128().unwrap_or_default(),
                },
            }),
            _ => Some(ValueArg::Binary(
                ArithOp::Sub,
                Box::new(ValueArg::Int(0)),
                Box::new(written_value(inner)?),
            )),
        },
        ExpressionKind::Unary(UnaryOp::Plus, inner) => written_value(inner),
        ExpressionKind::Binary(l, op, r) => Some(ValueArg::Binary(
            ArithOp::of(op)?,
            Box::new(written_value(l)?),
            Box::new(written_value(r)?),
        )),
        _ => None,
    }
}
