// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Operator type checking for the type checker.
//!
//! This module handles type validation for binary and unary operators,
//! ensuring operands have compatible types for the requested operations.

use super::context::{
    class_implements_trait, resolve_method_source, trait_is_or_extends, Context, TypeDefinition,
};
use super::TypeChecker;
use crate::ast::implicit_methods::operator_method_name;
use crate::ast::type_arguments::{argument_type, TypeArgKind, TypeArgument};
use crate::ast::types::{
    common_numeric_type, is_range_type_name, scalar_width, vec_dim, BuiltinCollectionKind, Type,
    TypeKind, ATOMIC_TYPE_NAME, EQUALS_METHOD_NAME, HASH_METHOD_NAME, ORDERING_TRAIT_NAME,
    STEPPING_TRAIT_NAME, STRING_TYPE_NAME,
};
use crate::ast::BinaryOp;
use crate::ast::UnaryOp;

/// Bound on how deeply structural equality may nest before the compiler
/// refuses. The comparison is expanded inline, so an unbounded chain would
/// exhaust the compiler's stack on user input.
const MAX_STRUCTURAL_EQUALITY_DEPTH: usize = 64;

/// True for the operators that ask which operand sorts first, as opposed to
/// whether the two operands are the same.
pub(crate) fn is_ordering_op(op: &BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::LessThan
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual
    )
}

/// Whether either operand of a binary operator is one of the body's type
/// parameters, whose pairing with the other operand each instantiation of the
/// body judges rather than the body itself.
/// The type an operator between two numbers of different types computes at,
/// or the refusal when no type holds every value of both; `None` when the two
/// are not both numbers of one family, which the caller judges.
fn common_number_type(left: &Type, right: &Type) -> Option<Result<Type, String>> {
    let both_numbers = scalar_width(&left.kind).is_some()
        && scalar_width(&right.kind).is_some()
        && !matches!(left.kind, TypeKind::Boolean)
        && !matches!(right.kind, TypeKind::Boolean);
    if !both_numbers || left.kind == right.kind {
        return None;
    }
    let float = |ty: &Type| {
        matches!(
            ty.kind,
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
        )
    };
    if float(left) != float(right) {
        return None;
    }
    Some(match common_numeric_type(&left.kind, &right.kind) {
        Some(common) => Ok(Type::new(common, left.span)),
        None => Err(format!(
            "Type mismatch: no number type holds every value of both '{left}' and '{right}'; \
             convert one operand with `as` so both are the same type"
        )),
    })
}

fn has_parameter_operand(left: &Type, right: &Type) -> bool {
    [left, right]
        .iter()
        .any(|operand| matches!(operand.kind, TypeKind::Generic(..)))
}

/// The message reported when an ordering operator is applied to a type that
/// defines no ordering.
pub(crate) fn missing_ordering_message(ty: &Type, op: &BinaryOp) -> String {
    format!(
        "Type '{}' has no ordering: '{}' requires the {} trait",
        ty,
        crate::ast::formatter::helpers::binary_operator(*op),
        ORDERING_TRAIT_NAME
    )
}

/// The message reported where a generic parameter a body orders is pinned to a
/// type that defines no ordering.
///
/// The comparison sits in a body written against the parameter, which may spell
/// it with any of the four ordering operators, so this names the capability
/// rather than one operator the reader cannot see from the call site.
pub(crate) fn missing_ordering_at_instantiation_message(ty: &Type) -> String {
    format!(
        "Type '{}' has no ordering: comparing it requires the {} trait",
        ty, ORDERING_TRAIT_NAME
    )
}

/// How the values of a type that orders are asked which of them sorts first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueOrdering {
    /// The machine compares the values themselves: numbers and `bool`.
    Machine,
    /// The type's own `compare`, which its `Comparable` implementation
    /// supplies, answers.
    Method,
    /// A generic parameter, ordered by whatever each instantiation pins it to.
    Deferred,
}

/// How a `for` loop steps from one value of a range to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueStepping {
    /// The machine adds one: the integers.
    Machine,
    /// The type's own `successor`, which its `Steppable` implementation
    /// supplies, gives the next value, and its `compare` says when the loop
    /// has reached the end.
    Method,
    /// A generic parameter bounded by `Steppable`, stepped the way each
    /// instantiation's type steps.
    Deferred,
}

/// Why a range of some type cannot be stepped through, which decides what a
/// refusal suggests instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum NoStep {
    /// A float has no next value.
    Float,
    /// `bool` has two values and no successor.
    Boolean,
    /// A generic parameter whose bound does not include `Steppable`.
    UnboundedParameter(String),
    /// A value typed at the trait it is named by, which can hold a value of
    /// any type implementing it, so two bounds need not step through each other.
    TraitTyped(String),
    /// Any other type that does not implement `Steppable`.
    NotSteppable,
}

/// The message reported when a `for` loop steps through a range of `element`
/// values, which cannot be stepped.
pub(crate) fn unsteppable_range_message(element: &Type) -> String {
    format!(
        "Range of '{element}' cannot be stepped: `for` steps through a range with its element \
         type's successor, which the {STEPPING_TRAIT_NAME} trait gives"
    )
}

/// The help for a refusal of a `for` loop over a range of `element` values,
/// which says what the loop can do instead. `function` names the function the
/// loop is written in, when it is written in one.
pub(crate) fn unsteppable_range_help(
    element: &Type,
    why: &NoStep,
    function: Option<&str>,
) -> String {
    match why {
        NoStep::Float => "iterate an integer range and scale it: `for i in 0..n` then \
             `let x = start + (i as float) * step`; `x in a..b` still tests membership in a \
             float range"
            .to_string(),
        NoStep::Boolean => "walk the two values in a list: `for b in [false, true]`; \
             `x in a..b` still tests membership in a bool range"
            .to_string(),
        NoStep::UnboundedParameter(parameter) => format!(
            "bound the parameter so every type it stands for can be stepped: {}",
            stepping_parameter(function, parameter)
        ),
        NoStep::TraitTyped(name) => format!(
            "a '{name}' may hold a value of any type implementing it, so take both bounds \
             at one type parameter instead: {}",
            stepping_parameter(function, "T")
        ),
        NoStep::NotSteppable => format!(
            "implement {STEPPING_TRAIT_NAME} on '{element}' with `public fn compare(other Self) \
             int` and `public fn successor() Self`, or loop over a list of the values; \
             `x in a..b` still tests membership in a range of any type `<` orders"
        ),
    }
}

/// The declaration of a type parameter bounded by `Steppable`, written on
/// `function` when the loop is written in one.
fn stepping_parameter(function: Option<&str>, parameter: &str) -> String {
    match function {
        Some(function) => {
            format!("`fn {function}<{parameter} implements {STEPPING_TRAIT_NAME}>(...)`")
        }
        None => format!("`<{parameter} implements {STEPPING_TRAIT_NAME}>`"),
    }
}

/// The class whose declaration answers which traits values of `kind`
/// implement: `String`'s for a string, the named type's own for a named type,
/// and none for a type no declaration stands behind.
pub(crate) fn named_class(kind: &TypeKind) -> Option<&str> {
    match kind {
        TypeKind::String => Some(STRING_TYPE_NAME),
        TypeKind::Custom(name, _) => Some(name.as_str()),
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
        | TypeKind::List(_)
        | TypeKind::Array(..)
        | TypeKind::Map(..)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(..)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(..)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_)
        | TypeKind::OneOf(_) => None,
    }
}

/// Why a range membership test `value in r` is refused.
pub(crate) enum RangeMembershipRefusal {
    /// The value and the bounds are of types that cannot be compared; the
    /// message says which.
    Mismatch(String),
    /// The value and the bounds agree, but this type of theirs has no
    /// ordering.
    Unordered(Type),
    /// Inside a kernel, the bounds are of a type no device value has.
    OffDevice,
}

/// The message for a membership test of a `value` that cannot be compared
/// with the `element` bounds of the range.
fn membership_mismatch_message(value: &Type, element: &Type) -> String {
    format!("Type mismatch: cannot check membership of {value} in range of {element}")
}

/// The message reported when a range membership test is applied to a value,
/// or a range of values, of a type that defines no ordering.
pub(crate) fn missing_range_ordering_message(ty: &Type) -> String {
    format!(
        "Type '{ty}' has no ordering: testing it against a range's bounds requires the {ORDERING_TRAIT_NAME} trait"
    )
}

impl TypeChecker {
    /// Checks that binary operation operands have compatible types.
    ///
    /// Returns the result type of the operation, or an error message if
    /// the operands are incompatible.
    pub(crate) fn check_binary_op_types(
        &mut self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                self.check_arithmetic_op(left, op, right, context)
            }
            BinaryOp::Equal | BinaryOp::NotEqual => self.check_equality_op(left, right, context),
            BinaryOp::LessThan
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual => self.check_ordering_op(left, op, right, context),
            BinaryOp::And | BinaryOp::Or => self.check_logical_op(left, right),
            BinaryOp::BitwiseAnd | BinaryOp::BitwiseOr | BinaryOp::BitwiseXor => {
                self.check_bitwise_op(left, right, context)
            }
            BinaryOp::In => self.check_membership_op(left, right, context),
            BinaryOp::NullCoalesce => self.check_null_coalesce_op(left, right, context),
            BinaryOp::Not | BinaryOp::Range => {
                Ok(crate::ast::factory::make_type(TypeKind::Boolean))
            }
        }
    }

    /// Checks arithmetic operations (+, -, *, /, %).
    ///
    /// Takes the checker by shared reference so the judgment can be replayed
    /// where a generic parameter is pinned to a concrete type, against the
    /// types that pinning produces.
    pub(crate) fn check_arithmetic_op(
        &self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let left_is_int = self.is_integer(left);
        let left_is_float = matches!(
            left.kind,
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
        );
        let right_is_int = self.is_integer(right);
        let right_is_float = matches!(
            right.kind,
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
        );

        // Disallow mixed int/float operations
        if (left_is_int && right_is_float) || (left_is_float && right_is_int) {
            let op_name = match op {
                BinaryOp::Add => "add",
                BinaryOp::Sub => "subtract",
                BinaryOp::Mul => "multiply",
                BinaryOp::Div => "divide",
                BinaryOp::Mod => "modulo",
                _ => "operate on",
            };
            return Err(format!(
                "Type mismatch: cannot {} a float to an integer",
                op_name
            ));
        }

        // Two numbers compute at a type that holds every value of both, so
        // neither is truncated nor wrapped, and `a + b` is `b + a`.
        if let Some(result) = common_number_type(left, right) {
            return result;
        }

        // Numeric operations
        if self.is_numeric(left) && self.is_numeric(right) {
            if self.are_compatible(left, right, context) {
                return Ok(left.clone());
            }
            return Err(format!(
                "Type mismatch: {} and {} are not compatible for arithmetic operation",
                left, right
            ));
        }

        // A parameter has no type to judge here. The body states what it applies
        // to the parameter, and every site that pins it answers for the type it
        // pins it to — see `instantiation_requirements`. The requirement carries
        // both operands, so the one beside the parameter — a literal, a
        // concrete value, a second parameter — is judged with it there.
        //
        // The result is the parameter operand's type: `k * a` with `k int` is
        // an `a`, whichever side it is written on. Each site checks that the
        // operator it judges gives the type the body read, so a binding whose
        // values `int` would widen (`a` an `i8`) is refused there.
        if has_parameter_operand(left, right) {
            let parameter = if matches!(left.kind, TypeKind::Generic(..)) {
                left
            } else {
                right
            };
            return Ok(parameter.clone());
        }

        // Trait-based Add: if left implements Addable and types are compatible
        if matches!(op, BinaryOp::Add) && self.implements_operator_trait(left, &BinaryOp::Add) {
            if self.are_compatible(left, right, context) {
                return Ok(left.clone());
            }
            return Err(format!(
                "Type mismatch: cannot add {} and {} (both must be the same type)",
                left, right
            ));
        }
        // Trait-based Mul: if left implements Multiplicable and right is int
        if matches!(op, BinaryOp::Mul) && self.implements_operator_trait(left, &BinaryOp::Mul) {
            if self.is_numeric(right) {
                return Ok(left.clone());
            }
            return Err(format!(
                "Type mismatch: cannot multiply {} by {} (right operand must be an integer)",
                left, right
            ));
        }

        // Vector-scalar broadcast: allow operations like Vec3<f32> * f32
        if let TypeKind::Custom(vec_name, Some(args)) = &left.kind {
            if vec_dim(vec_name).is_some() && self.is_numeric(right) {
                if let Some(first_arg) = args.first() {
                    if let crate::ast::type_arguments::TypeArgKind::Type(elem_type, _) =
                        &first_arg.node
                    {
                        if self.is_numeric(elem_type) {
                            return Ok(left.clone());
                        }
                    }
                }
            }
        }

        // Reverse vector-scalar broadcast: allow operations like f32 * Vec3<f32>
        if let TypeKind::Custom(vec_name, Some(args)) = &right.kind {
            if vec_dim(vec_name).is_some() && self.is_numeric(left) {
                if let Some(first_arg) = args.first() {
                    if let crate::ast::type_arguments::TypeArgKind::Type(elem_type, _) =
                        &first_arg.node
                    {
                        if self.is_numeric(elem_type) {
                            return Ok(right.clone());
                        }
                    }
                }
            }
        }

        Err(format!(
            "Invalid types for arithmetic operation: {} and {}",
            left, right
        ))
    }

    /// Checks equality operations (==, !=).
    ///
    /// Equality asks whether two values are the same, which every type can
    /// answer: numerically, structurally, or through an `Equatable`
    /// implementation of its own.
    pub(crate) fn check_equality_op(
        &self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let bool_type = || crate::ast::factory::make_type(TypeKind::Boolean);

        // Allow comparison between any integers
        if self.is_integer(left) && self.is_integer(right) {
            return Ok(bool_type());
        }

        // Allow comparison between any floats
        if matches!(left.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64)
            && matches!(right.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64)
        {
            return Ok(bool_type());
        }

        if has_parameter_operand(left, right) {
            return Ok(bool_type());
        }

        // Allow comparison between compatible types
        if self.are_compatible(left, right, context) {
            self.check_type_structurally_comparable(left)?;
            return Ok(bool_type());
        }

        // Trait-based Equatable: if left implements Equatable
        if self.implements_operator_trait(left, &BinaryOp::Equal)
            && self.are_compatible(left, right, context)
        {
            return Ok(bool_type());
        }

        Err(format!(
            "Type mismatch: cannot compare {} and {}",
            left, right
        ))
    }

    /// Checks ordering operations (<, <=, >, >=).
    ///
    /// Ordering asks which of two values sorts first, which is a capability a
    /// type has to define. Without one there is no answer to fall back on:
    /// comparing two objects by address answers from allocation order, so the
    /// same two values would compare differently depending on which was built
    /// first. A type that defines no ordering is therefore refused.
    pub(crate) fn check_ordering_op(
        &self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let bool_type = || crate::ast::factory::make_type(TypeKind::Boolean);

        if let Some(message) = self.missing_ordering(left, op, right, context) {
            return Err(message);
        }

        // Numbers order by value, across widths and across signedness.
        if self.is_integer(left) && self.is_integer(right) {
            return Ok(bool_type());
        }
        if matches!(left.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64)
            && matches!(right.kind, TypeKind::Float | TypeKind::F32 | TypeKind::F64)
        {
            return Ok(bool_type());
        }

        if !has_parameter_operand(left, right) && !self.are_compatible(left, right, context) {
            return Err(format!(
                "Type mismatch: cannot compare {} and {}",
                left, right
            ));
        }

        Ok(bool_type())
    }

    /// The message for an ordering operator applied to two operands that agree
    /// on type but whose type defines no ordering, or None when that is not
    /// what is wrong.
    ///
    /// The check that refuses the operation and the diagnosis that explains it
    /// both ask this, so the two cannot come to disagree about when the
    /// capability is missing.
    pub(crate) fn missing_ordering(
        &self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Option<String> {
        if !is_ordering_op(op) {
            return None;
        }
        // Beside a parameter, whose ordering is answered where it is bound,
        // the other operand's type is already known and must order itself:
        // `b < a` with `b Dog` has no ordering at any binding of `a`.
        if has_parameter_operand(left, right) {
            return [left, right]
                .into_iter()
                .find(|operand| !self.orders_its_values(operand))
                .map(|unordered| missing_ordering_message(unordered, op));
        }
        if self.orders_its_values(left) {
            return None;
        }
        if !self.are_compatible(left, right, context) {
            return None;
        }
        Some(missing_ordering_message(left, op))
    }

    /// True when values of `ty` can be asked which of them sorts first.
    pub(crate) fn orders_its_values(&self, ty: &Type) -> bool {
        self.ordering_of(ty).is_some()
    }

    /// How values of `ty` are asked which of them sorts first, or `None` when
    /// they cannot be. The checker refuses an ordering by it, and range
    /// membership is lowered by it.
    ///
    /// The numeric types and `bool` order by value: `false` sorts before
    /// `true`, and the comparison the machine performs is over the value the
    /// operand holds rather than over where it lives. A string or a named type
    /// orders through the `compare` its `Comparable` implementation supplies.
    /// A generic parameter is accepted the way it is for arithmetic: the body
    /// of a generic function is checked once, and the capability is decided
    /// where the parameter is pinned to a concrete type.
    pub(crate) fn ordering_of(&self, ty: &Type) -> Option<ValueOrdering> {
        match &ty.kind {
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
            | TypeKind::Boolean => Some(ValueOrdering::Machine),
            TypeKind::String | TypeKind::Custom(..) => self
                .type_implements_trait(ty, ORDERING_TRAIT_NAME)
                .then_some(ValueOrdering::Method),
            TypeKind::Generic(..) => Some(ValueOrdering::Deferred),
            // The collection spellings only reach the checker before
            // normalization turns them into named types, so none orders.
            TypeKind::List(_)
            | TypeKind::Array(..)
            | TypeKind::Map(..)
            | TypeKind::Set(_)
            | TypeKind::Option(_)
            | TypeKind::Tuple(_)
            | TypeKind::Function(_)
            | TypeKind::Result(..)
            | TypeKind::Future(_)
            | TypeKind::Linear(_)
            | TypeKind::Meta(_)
            | TypeKind::Void
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::OneOf(_)
            | TypeKind::Error => None,
        }
    }

    /// How a `for` loop steps through a range of `ty` values, or why it
    /// cannot. The checker refuses a loop by it, and the loop is lowered by
    /// it.
    ///
    /// The integers add one. A type implementing `Steppable` — `String`, or a
    /// class — steps with its own `successor` and orders with its own
    /// `compare`. A generic parameter steps when its bound is `Steppable` or
    /// a trait extending it; the body is checked once, so an unbounded one
    /// is refused where it is written rather than at each instantiation.
    pub(crate) fn range_step(&self, ty: &Type) -> Result<ValueStepping, NoStep> {
        match &ty.kind {
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64 => Err(NoStep::Float),
            TypeKind::Boolean => Err(NoStep::Boolean),
            TypeKind::Generic(name, bound, _) => {
                if bound
                    .as_deref()
                    .is_some_and(|bound| self.bound_steps(bound))
                {
                    Ok(ValueStepping::Deferred)
                } else {
                    Err(NoStep::UnboundedParameter(name.clone()))
                }
            }
            TypeKind::Custom(name, _) if self.names_a_trait(name) => {
                Err(NoStep::TraitTyped(name.clone()))
            }
            TypeKind::String | TypeKind::Custom(..) => {
                if self.type_implements_trait(ty, STEPPING_TRAIT_NAME) {
                    Ok(ValueStepping::Method)
                } else {
                    Err(NoStep::NotSteppable)
                }
            }
            // The integers meet `Steppable` without declaring it: the machine
            // steps one by adding one.
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
            | TypeKind::U128 => Ok(ValueStepping::Machine),
            TypeKind::List(_)
            | TypeKind::Array(..)
            | TypeKind::Map(..)
            | TypeKind::Set(_)
            | TypeKind::Option(_)
            | TypeKind::Tuple(_)
            | TypeKind::Function(_)
            | TypeKind::Result(..)
            | TypeKind::Future(_)
            | TypeKind::Linear(_)
            | TypeKind::Meta(_)
            | TypeKind::Void
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::OneOf(_)
            | TypeKind::Error => Err(NoStep::NotSteppable),
        }
    }

    /// Whether values of `ty` meet `Steppable` by value, stepped by the
    /// machine adding one rather than through a `successor` of their own.
    pub(crate) fn steps_by_machine(&self, ty: &Type) -> bool {
        self.range_step(ty) == Ok(ValueStepping::Machine)
    }

    /// Whether `name` names a trait.
    fn names_a_trait(&self, name: &str) -> bool {
        matches!(
            self.type_table.global_type_definitions.get(name),
            Some(crate::type_checker::context::TypeDefinition::Trait(_))
        )
    }

    /// Whether a parameter bounded by `bound` steps: the bound is
    /// `Steppable`, a trait extending it, or a class implementing it.
    fn bound_steps(&self, bound: &Type) -> bool {
        let TypeKind::Custom(name, _) = &bound.kind else {
            return false;
        };
        let definitions = &self.type_table.global_type_definitions;
        trait_is_or_extends(name, STEPPING_TRAIT_NAME, definitions)
            || class_implements_trait(name, STEPPING_TRAIT_NAME, definitions)
    }

    /// Checks logical operations (&&, ||).
    fn check_logical_op(&self, left: &Type, right: &Type) -> Result<Type, String> {
        if matches!(left.kind, TypeKind::Boolean) && matches!(right.kind, TypeKind::Boolean) {
            Ok(crate::ast::factory::make_type(TypeKind::Boolean))
        } else {
            Err(format!(
                "Logical operations require booleans, got {} and {}",
                left, right
            ))
        }
    }

    /// Checks bitwise operations (&, |, ^).
    fn check_bitwise_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        if !self.is_integer(left) || !self.is_integer(right) {
            return Err(format!(
                "Invalid types for bitwise operation: {} and {}",
                left, right
            ));
        }
        if let Some(result) = common_number_type(left, right) {
            return result;
        }

        if left == right || matches!(right.kind, TypeKind::Int) {
            return Ok(left.clone());
        }

        if matches!(left.kind, TypeKind::Int) && self.are_compatible(right, left, context) {
            return Ok(right.clone());
        }

        Err(format!(
            "Type mismatch: {} and {} are not compatible for bitwise operation",
            left, right
        ))
    }

    /// Checks membership operation (`in`).
    fn check_membership_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        let bool_type = || crate::ast::factory::make_type(TypeKind::Boolean);

        match &right.kind {
            // Canonical collection variants are normalized to Custom before this point.
            TypeKind::List(_) | TypeKind::Set(_) | TypeKind::Map(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Custom(name, Some(args))
                if matches!(
                    BuiltinCollectionKind::from_name(name.as_str()),
                    Some(
                        BuiltinCollectionKind::List
                            | BuiltinCollectionKind::Array
                            | BuiltinCollectionKind::Set
                    )
                ) && !args.is_empty() =>
            {
                let inner = self.resolve_type_argument(&args[0], context);
                if self.are_compatible(&inner, left, context) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in collection of {}",
                        left, inner
                    ))
                }
            }
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name.as_str())
                    == Some(BuiltinCollectionKind::Map)
                    && !args.is_empty() =>
            {
                let key = self.resolve_type_argument(&args[0], context);
                if self.are_compatible(&key, left, context) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in map with keys of {}",
                        left, key
                    ))
                }
            }
            TypeKind::Custom(name, Some(args)) if is_range_type_name(name) && args.len() == 1 => {
                let element = self.resolve_type_argument(&args[0], context);
                self.check_range_membership(left, &element, context)
            }
            TypeKind::String => {
                if matches!(left.kind, TypeKind::String) {
                    Ok(bool_type())
                } else {
                    Err(format!(
                        "Type mismatch: cannot check membership of {} in String (expected String)",
                        left
                    ))
                }
            }
            _ => Err(format!(
                "Invalid type for 'in' operator: expected collection, got {}",
                right
            )),
        }
    }

    /// Checks `x in r` on a range of `element`. It stands for `r.start <= x`
    /// and `x < r.end`, so it accepts exactly the operands those comparisons
    /// accept, each with the operands in the order it is made in.
    fn check_range_membership(
        &self,
        left: &Type,
        element: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        self.check_ordering_op(element, &BinaryOp::LessThanEqual, left, context)
            .and_then(|_| self.check_ordering_op(left, &BinaryOp::LessThan, element, context))
            .map_err(|_| membership_mismatch_message(left, element))
    }

    /// What is wrong with `value in r` on a range of `element`, or `None`
    /// when nothing is.
    ///
    /// Two types that do not agree are a mismatch before either is asked for
    /// an ordering, so `p in 1..3` names the disagreement rather than the
    /// ordering `p` lacks. Inside a kernel the element must be a value the
    /// device holds.
    pub(crate) fn range_membership_refusal(
        &self,
        value: &Type,
        element: &Type,
        context: &Context,
    ) -> Option<RangeMembershipRefusal> {
        let unordered = [value, element]
            .into_iter()
            .find(|ty| !self.orders_its_values(ty));
        if let Some(unordered) = unordered {
            return Some(if self.range_operands_agree(value, element, context) {
                RangeMembershipRefusal::Unordered(unordered.clone())
            } else {
                RangeMembershipRefusal::Mismatch(membership_mismatch_message(value, element))
            });
        }
        if let Err(message) = self.check_range_membership(value, element, context) {
            return Some(RangeMembershipRefusal::Mismatch(message));
        }
        let off_device = context.in_gpu_function
            && !crate::type_checker::utils::is_gpu_compatible(&element.kind);
        off_device.then_some(RangeMembershipRefusal::OffDevice)
    }

    /// Whether the value and the bounds of a range membership test are of
    /// types one of which holds the other, ordering aside.
    fn range_operands_agree(&self, value: &Type, element: &Type, context: &Context) -> bool {
        has_parameter_operand(value, element)
            || self.are_compatible(element, value, context)
            || self.are_compatible(value, element, context)
    }

    /// The element type of the range `right` that `x in right` tests `x`
    /// against, or `None` when `op` is not `in` or `right` is not a range.
    pub(crate) fn range_membership_element(
        &mut self,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Option<Type> {
        if !matches!(op, BinaryOp::In) {
            return None;
        }
        let TypeKind::Custom(name, Some(args)) = &right.kind else {
            return None;
        };
        match args.as_slice() {
            [element] if is_range_type_name(name) => {
                Some(self.resolve_type_argument(element, context))
            }
            _ => None,
        }
    }

    /// Checks unary operation operand types.
    ///
    /// Returns the result type of the operation, or an error message if
    /// the operand is incompatible.
    pub(crate) fn check_unary_op_types(
        &self,
        op: &UnaryOp,
        expr_type: &Type,
    ) -> Result<Type, String> {
        match op {
            // A parameter has no type to judge here, the way it has none for
            // binary arithmetic: the body states that it negates the parameter,
            // and every site that pins it answers for the type it pins it to —
            // see `instantiation_requirements`.
            UnaryOp::Negate | UnaryOp::Plus => {
                if self.is_numeric(expr_type) || matches!(expr_type.kind, TypeKind::Generic(..)) {
                    Ok(expr_type.clone())
                } else {
                    Err(format!(
                        "Unary operator requires numeric type, got {}",
                        expr_type
                    ))
                }
            }
            // `++x` and `--x` are not admitted on a parameter. `++x` answers
            // the operand unchanged and `--x` lowers as a double negation, so
            // each would need its own account of what an instantiation owes
            // before it could be deferred the way negation is.
            UnaryOp::Decrement | UnaryOp::Increment => {
                if self.is_numeric(expr_type) {
                    Ok(expr_type.clone())
                } else {
                    Err(format!(
                        "Unary operator requires numeric type, got {}",
                        expr_type
                    ))
                }
            }
            UnaryOp::Not => {
                if matches!(expr_type.kind, TypeKind::Boolean) {
                    Ok(crate::ast::factory::make_type(TypeKind::Boolean))
                } else {
                    Err(format!("Logical NOT requires boolean, got {}", expr_type))
                }
            }
            UnaryOp::Await => {
                if let TypeKind::Future(inner_expr) = &expr_type.kind {
                    self.extract_type_from_argument(inner_expr)
                } else if let TypeKind::Custom(name, args) = &expr_type.kind {
                    if name == "Future" {
                        if let Some(args) = args {
                            if let Some(arg) = args.first() {
                                return self.extract_type_from_argument(arg);
                            }
                        }
                        return Ok(crate::ast::factory::make_type(TypeKind::Void));
                    }
                    Err(format!("Await requires a Future, got {}", expr_type))
                } else {
                    Err(format!("Await requires a Future, got {}", expr_type))
                }
            }
            UnaryOp::BitwiseNot => {
                if self.is_integer(expr_type) {
                    Ok(expr_type.clone())
                } else {
                    Err(format!(
                        "Bitwise NOT requires integer type, got {}",
                        expr_type
                    ))
                }
            }
        }
    }

    /// Checks null coalescing operation (`??`).
    /// LHS must be `Option<T>`, RHS must be compatible with `T`. Result type is `T`.
    fn check_null_coalesce_op(
        &mut self,
        left: &Type,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        if let TypeKind::Option(inner) = &left.kind {
            let inner_ty = inner.as_ref().clone();
            if self.are_compatible(&inner_ty, right, context) {
                Ok(inner_ty)
            } else {
                Err(format!(
                    "Type mismatch in '??': Option contains {}, but default value is {}",
                    inner_ty, right
                ))
            }
        } else {
            Err(format!(
                "Left side of '??' must be an Option type, got {}",
                left
            ))
        }
    }

    /// Whether `ty` implements the trait whose method answers `op`, read from
    /// the one operator table lowering dispatches by.
    fn implements_operator_trait(&self, ty: &Type, op: &BinaryOp) -> bool {
        crate::ast::implicit_methods::operator_trait_name(op)
            .is_some_and(|trait_name| self.type_implements_trait(ty, trait_name))
    }

    /// Whether a type implements a given trait: its class, or a class it
    /// extends, lists the trait or a trait that extends it. A type no class
    /// declaration stands behind implements none.
    fn type_implements_trait(&self, ty: &Type, trait_name: &str) -> bool {
        named_class(&ty.kind).is_some_and(|class_name| {
            class_implements_trait(
                class_name,
                trait_name,
                &self.type_table.global_type_definitions,
            )
        })
    }

    /// Rejects a type whose shape cannot be compared structurally.
    ///
    /// Structural equality is expanded inline at lowering, so a type that
    /// contains itself would expand without bound and take the compiler with
    /// it. A type that defines its own `equals` is always accepted: that
    /// method replaces the derived comparison, which is what makes the advice
    /// in these diagnostics work.
    pub(crate) fn check_type_structurally_comparable(&self, ty: &Type) -> Result<(), String> {
        let mut visiting = Vec::new();
        self.check_structural_comparability(&ty.kind, &mut visiting, 0)
    }

    /// Walk a type's payloads, carrying the enclosing type names to detect a
    /// cycle and the nesting depth to bound an acyclic chain. Both are passed
    /// down rather than held in shared state so sibling payloads cannot be
    /// mistaken for nesting.
    fn check_structural_comparability(
        &self,
        kind: &TypeKind,
        visiting: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), String> {
        if depth >= MAX_STRUCTURAL_EQUALITY_DEPTH {
            return Err(
                "Type nesting too deep for structural equality; implement `equals` method instead"
                    .to_string(),
            );
        }
        self.check_kind_comparability(kind, visiting, depth)
    }

    /// The comparability of a `kind` value, judged by its shape: a type
    /// holding values of other types is comparable when they all are.
    fn check_kind_comparability(
        &self,
        kind: &TypeKind,
        visiting: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), String> {
        match kind {
            TypeKind::Option(inner) => {
                self.check_structural_comparability(&inner.kind, visiting, depth + 1)
            }
            TypeKind::Result(ok, err) => {
                self.check_members_comparability([ok.as_ref(), err.as_ref()], visiting, depth)
            }
            // A tuple is compared member by member, so each member must be
            // comparable itself.
            TypeKind::Tuple(members) => self.check_members_comparability(members, visiting, depth),
            // A built-in collection is a type holding its elements, keys and
            // values, so it is refused whenever one of them is.
            TypeKind::Custom(name, Some(args))
                if BuiltinCollectionKind::from_name(name).is_some() =>
            {
                self.check_members_comparability(args, visiting, depth)
            }
            TypeKind::Custom(name, args) => {
                self.check_named_type_comparability(name, args.as_deref(), visiting, depth)
            }
            TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
                self.check_members_comparability([element.as_ref()], visiting, depth)
            }
            TypeKind::Array(element, _) => {
                self.check_members_comparability([element.as_ref()], visiting, depth)
            }
            TypeKind::Map(key, value) => {
                self.check_members_comparability([key.as_ref(), value.as_ref()], visiting, depth)
            }
            TypeKind::OneOf(members) => self.check_members_comparability(members, visiting, depth),
            TypeKind::Linear(inner) => {
                self.check_structural_comparability(&inner.kind, visiting, depth + 1)
            }
            // Two function values carry code and captures, not a value `==`
            // could compare: one lambda written twice is two functions, and
            // comparing addresses would answer from allocation order.
            TypeKind::Function(_) => Err(
                "Type mismatch: function values cannot be compared with `==`; compare what \
                 they compute instead"
                    .to_string(),
            ),
            // A type parameter is judged where it is bound; the rest hold no
            // value of another type.
            TypeKind::Generic(..)
            | TypeKind::Meta(_)
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
            | TypeKind::Error => Ok(()),
        }
    }

    /// Check that every type among `members`, the parts of a type `==`
    /// compares one by one, is comparable itself.
    fn check_members_comparability<'m>(
        &self,
        members: impl IntoIterator<Item = &'m TypeArgument>,
        visiting: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), String> {
        members
            .into_iter()
            .filter_map(argument_type)
            .try_for_each(|ty| self.check_structural_comparability(&ty.kind, visiting, depth + 1))
    }

    /// Whether `==` on a value of `kind` whose type declares no `equals`
    /// compares it member by member: an enum, a struct, a tuple, an optional
    /// or a result. A tuple
    /// compares each member at the member's own type, so a counted enum is
    /// compared by its payload and a class through its `equals`.
    ///
    /// An `Atomic<T>` is declared as a struct but has no struct behind it to
    /// walk; host code never compares one. This is the one answer: the
    /// lowering of `==` and the checker's own questions about it read it.
    pub fn compares_member_by_member(&self, kind: &TypeKind) -> bool {
        match kind {
            TypeKind::Custom(name, _) if name == ATOMIC_TYPE_NAME => false,
            TypeKind::Custom(name, _) => matches!(
                self.type_table.global_type_definitions.get(name),
                Some(TypeDefinition::Enum(_) | TypeDefinition::Struct(_))
            ),
            TypeKind::Result(..) | TypeKind::Option(_) | TypeKind::Tuple(_) => true,
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
            | TypeKind::List(_)
            | TypeKind::Array(..)
            | TypeKind::Map(..)
            | TypeKind::Set(_)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Generic(..)
            | TypeKind::Meta(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_)
            | TypeKind::OneOf(_) => false,
        }
    }

    /// The members a derived `==` on a `name` value reads, at the type
    /// arguments `args`: an enum's variant payloads, substituted, and a
    /// struct's fields as declared. `None` for a type `==` does not compare
    /// member by member.
    fn derived_equality_members(
        &self,
        name: &str,
        args: Option<&[TypeArgument]>,
    ) -> Option<Vec<Type>> {
        match self.type_table.global_type_definitions.get(name)? {
            TypeDefinition::Enum(enum_def) => Some(
                (enum_def.variants.values().flatten())
                    .map(|payload_ty| {
                        Type::new(
                            crate::type_checker::generics::substitute_generic_field_kind(
                                &payload_ty.kind,
                                args,
                                enum_def.generics.as_ref(),
                            ),
                            payload_ty.span,
                        )
                    })
                    .collect(),
            ),
            TypeDefinition::Struct(struct_def) => Some(
                struct_def
                    .fields
                    .iter()
                    .map(|field| field.1.clone())
                    .collect(),
            ),
            TypeDefinition::Class(_)
            | TypeDefinition::Trait(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_) => None,
        }
    }

    /// Check a named type's payloads or fields, unless it defines `equals`.
    fn check_named_type_comparability(
        &self,
        name: &str,
        args: Option<&[TypeArgument]>,
        visiting: &mut Vec<String>,
        depth: usize,
    ) -> Result<(), String> {
        if self.type_supplies_equality(name) {
            return Ok(());
        }
        if visiting.iter().any(|seen| seen == name) {
            return Err(format!(
                "Recursive type `{}` cannot use structural equality; implement `equals` method instead",
                name
            ));
        }

        let Some(member_types) = self.derived_equality_members(name, args) else {
            return Ok(());
        };

        visiting.push(name.to_string());
        let result = member_types.iter().try_for_each(|member_ty| {
            self.check_structural_comparability(&member_ty.kind, visiting, depth + 1)
        });
        visiting.pop();
        result
    }

    /// The name of the type that keeps a value of `ty` from being hashed
    /// consistently with how `==` compares it, or `None` when every part of
    /// it can be.
    ///
    /// A hash is derived wherever equality is: a scalar, a string, an
    /// optional, a tuple, struct or enum compared member by member, and a class
    /// without `equals`, compared by identity. A type that writes its own
    /// `equals` decides alone which of its values are equal, so only a `hash`
    /// of its own can agree with it, and one without is the answer. A type
    /// parameter is answered where it is pinned.
    pub(crate) fn type_lacking_hash(&self, ty: &Type) -> Option<String> {
        self.part_lacking_hash(&ty.kind, &mut Vec::new())
    }

    fn part_lacking_hash(&self, kind: &TypeKind, visiting: &mut Vec<String>) -> Option<String> {
        match kind {
            TypeKind::Option(inner) | TypeKind::Linear(inner) => {
                self.part_lacking_hash(&inner.kind, visiting)
            }
            TypeKind::Result(ok, err) => {
                self.arguments_lacking_hash([ok.as_ref(), err.as_ref()], visiting)
            }
            // A tuple is compared and hashed member by member, so a member
            // that cannot be hashed consistently with its `==` leaves the
            // whole tuple without one.
            TypeKind::Tuple(members) => self.arguments_lacking_hash(members, visiting),
            TypeKind::Custom(name, args) => {
                self.named_type_lacking_hash(name, args.as_deref(), visiting)
            }
            _ => None,
        }
    }

    fn arguments_lacking_hash<'a>(
        &self,
        arguments: impl IntoIterator<Item = &'a TypeArgument>,
        visiting: &mut Vec<String>,
    ) -> Option<String> {
        arguments
            .into_iter()
            .find_map(|argument| match &argument.node {
                crate::ast::type_arguments::TypeArgKind::Type(ty, _) => {
                    self.part_lacking_hash(&ty.kind, visiting)
                }
                _ => None,
            })
    }

    fn named_type_lacking_hash(
        &self,
        name: &str,
        args: Option<&[TypeArgument]>,
        visiting: &mut Vec<String>,
    ) -> Option<String> {
        if self.type_declares_method(name, HASH_METHOD_NAME) {
            return None;
        }
        if self.type_supplies_equality(name) {
            return Some(name.to_string());
        }
        if visiting.iter().any(|seen| seen == name) {
            return None;
        }
        let member_types = self.derived_equality_members(name, args)?;
        visiting.push(name.to_string());
        let lacking = member_types
            .iter()
            .find_map(|member| self.part_lacking_hash(&member.kind, visiting));
        visiting.pop();
        lacking
    }

    /// Whether the class or enum `name` declares `method` itself, or a class
    /// or trait it takes methods from does.
    pub(crate) fn type_declares_method(&self, name: &str, method: &str) -> bool {
        let definitions = &self.type_table.global_type_definitions;
        match definitions.get(name) {
            Some(TypeDefinition::Class(_)) => {
                resolve_method_source(definitions, name, method).is_some()
            }
            Some(TypeDefinition::Enum(enum_def)) => enum_def.methods.contains_key(method),
            _ => false,
        }
    }

    /// Whether `==` on a `name` value calls an `equals` its type supplies,
    /// declared or inherited from a trait default, rather than comparing
    /// members.
    fn type_supplies_equality(&self, name: &str) -> bool {
        self.type_declares_method(name, EQUALS_METHOD_NAME)
    }

    /// Whether `op` applied to a left operand of type `left` may run a method
    /// body: the one the operator table names, as the left operand's type
    /// answers it, or for an equality an `equals` some part of a value
    /// compared member by member declares. A membership test compares the
    /// left operand against each element of a collection the way `==` does,
    /// and against the bounds of a range the way `<` does. A type parameter
    /// may stand for a type that answers.
    pub(crate) fn operator_may_run_method(&self, left: &Type, op: &BinaryOp) -> bool {
        if matches!(op, BinaryOp::In) {
            return self.operator_may_run_method(left, &BinaryOp::Equal)
                || self.operator_may_run_method(left, &BinaryOp::LessThan);
        }
        let Some(method) = operator_method_name(op) else {
            return false;
        };
        if method == EQUALS_METHOD_NAME {
            return self.equality_may_run_method(&left.kind, &mut Vec::new());
        }
        let name = match &left.kind {
            TypeKind::String => STRING_TYPE_NAME,
            TypeKind::Custom(name, _) => name.as_str(),
            kind => return matches!(kind, TypeKind::Generic(..)),
        };
        self.type_may_answer(name, method)
    }

    /// Whether comparing two values of `kind` with `==` may run an `equals`:
    /// one a part's type declares, reached through every member a derived
    /// comparison reads. `visiting` holds the named types the walk is inside.
    fn equality_may_run_method(&self, kind: &TypeKind, visiting: &mut Vec<String>) -> bool {
        match kind {
            TypeKind::Generic(..) => true,
            TypeKind::String => self.type_may_answer(STRING_TYPE_NAME, EQUALS_METHOD_NAME),
            TypeKind::Option(inner) | TypeKind::Linear(inner) | TypeKind::Meta(inner) => {
                self.equality_may_run_method(&inner.kind, visiting)
            }
            TypeKind::List(element) | TypeKind::Set(element) | TypeKind::Future(element) => {
                self.arguments_may_run_equals([element.as_ref()], visiting)
            }
            TypeKind::Array(first, second)
            | TypeKind::Map(first, second)
            | TypeKind::Result(first, second) => {
                self.arguments_may_run_equals([first.as_ref(), second.as_ref()], visiting)
            }
            TypeKind::Tuple(parts) | TypeKind::OneOf(parts) => {
                self.arguments_may_run_equals(parts, visiting)
            }
            TypeKind::Custom(name, args) => {
                self.named_equality_may_run_method(kind, (name, args.as_deref()), visiting)
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
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Function(_)
            | TypeKind::Void
            | TypeKind::Error => false,
        }
    }

    /// Whether comparing the parts the type arguments `arguments` name may
    /// run an `equals`.
    fn arguments_may_run_equals<'a>(
        &self,
        arguments: impl IntoIterator<Item = &'a TypeArgument>,
        visiting: &mut Vec<String>,
    ) -> bool {
        arguments.into_iter().any(|argument| match &argument.node {
            TypeArgKind::Type(ty, _) => self.equality_may_run_method(&ty.kind, visiting),
            TypeArgKind::Value(_) => false,
        })
    }

    /// Whether comparing two values of the named type `kind` may run an
    /// `equals`: its own, or one a member reaches when it is compared member
    /// by member. Any other named type — a class without one — is compared
    /// by identity.
    fn named_equality_may_run_method(
        &self,
        kind: &TypeKind,
        (name, args): (&str, Option<&[TypeArgument]>),
        visiting: &mut Vec<String>,
    ) -> bool {
        if self.type_may_answer(name, EQUALS_METHOD_NAME) {
            return true;
        }
        if !self.compares_member_by_member(kind) || visiting.iter().any(|seen| seen == name) {
            return false;
        }
        let Some(members) = self.derived_equality_members(name, args) else {
            return false;
        };
        visiting.push(name.to_string());
        let runs = members
            .iter()
            .any(|member| self.equality_may_run_method(&member.kind, visiting));
        visiting.pop();
        runs
    }

    /// Whether a value of the type `name` may answer `method` with a body:
    /// a class or enum that declares or inherits it, and a trait, type
    /// parameter, alias or unknown name, which may stand for one that does.
    fn type_may_answer(&self, name: &str, method: &str) -> bool {
        match self.type_table.global_type_definitions.get(name) {
            Some(TypeDefinition::Class(_) | TypeDefinition::Enum(_)) => {
                self.type_declares_method(name, method)
            }
            Some(TypeDefinition::Struct(_)) => false,
            Some(
                TypeDefinition::Trait(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_),
            )
            | None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::type_arguments::type_argument;
    use crate::error::syntax::Span;

    fn ty(kind: TypeKind) -> Type {
        Type::new(kind, Span::new(0, 0))
    }

    fn argument(kind: TypeKind) -> TypeArgument {
        type_argument(ty(kind))
    }

    /// A checker that has seen a class implementing `Comparable`, one
    /// implementing `Steppable` and one implementing neither, beside every
    /// declaration the prelude brings.
    fn checker() -> TypeChecker {
        let source = "
class Ranked implements Comparable
    var n int
    public fn compare(other Ranked) int
        return self.n - other.n

class Stepper implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other Stepper) int
        return self.n - other.n
    public fn successor() Stepper
        return Stepper(self.n + 1)

class Plain
    var n int

fn main()
    let a = 1
";
        let mut lexer = crate::lexer::Lexer::new(source);
        let mut parser = crate::parser::Parser::new(&mut lexer, source);
        let mut program = match parser.parse() {
            Ok(program) => program,
            Err(error) => panic!("the source does not parse: {error:?}"),
        };
        let mut checker = TypeChecker::new();
        if let Err(error) = checker.check(&mut program) {
            panic!("the source does not type-check: {error:?}");
        }
        checker
    }

    #[test]
    fn numbers_booleans_comparables_and_parameters_order() {
        let checker = checker();
        let machine = [
            TypeKind::Int,
            TypeKind::I8,
            TypeKind::I16,
            TypeKind::I32,
            TypeKind::I64,
            TypeKind::I128,
            TypeKind::U8,
            TypeKind::U16,
            TypeKind::U32,
            TypeKind::U64,
            TypeKind::U128,
            TypeKind::Float,
            TypeKind::F16,
            TypeKind::F32,
            TypeKind::F64,
            TypeKind::Boolean,
        ];
        for kind in machine {
            assert_eq!(
                checker.ordering_of(&ty(kind.clone())),
                Some(ValueOrdering::Machine),
                "{kind:?}"
            );
        }
        let method = [
            TypeKind::String,
            TypeKind::Custom("Ranked".to_string(), None),
        ];
        for kind in method {
            assert_eq!(
                checker.ordering_of(&ty(kind.clone())),
                Some(ValueOrdering::Method),
                "{kind:?}"
            );
        }
        let parameter = TypeKind::Generic(
            "T".to_string(),
            None,
            crate::ast::types::TypeDeclarationKind::None,
        );
        assert_eq!(
            checker.ordering_of(&ty(parameter)),
            Some(ValueOrdering::Deferred)
        );
    }

    #[test]
    fn every_other_type_kind_has_no_ordering() {
        let checker = checker();
        let unordered = [
            TypeKind::Custom("Plain".to_string(), None),
            TypeKind::List(Box::new(argument(TypeKind::Int))),
            TypeKind::Array(
                Box::new(argument(TypeKind::Int)),
                Box::new(argument(TypeKind::Int)),
            ),
            TypeKind::Map(
                Box::new(argument(TypeKind::Int)),
                Box::new(argument(TypeKind::Int)),
            ),
            TypeKind::Set(Box::new(argument(TypeKind::Int))),
            TypeKind::Option(Box::new(ty(TypeKind::Int))),
            TypeKind::Tuple(vec![argument(TypeKind::Int), argument(TypeKind::Int)]),
            TypeKind::Result(
                Box::new(argument(TypeKind::Int)),
                Box::new(argument(TypeKind::String)),
            ),
            TypeKind::Future(Box::new(argument(TypeKind::Int))),
            TypeKind::Function(Box::new(crate::ast::types::FunctionTypeData {
                generics: None,
                params: Vec::new(),
                return_type: None,
            })),
            TypeKind::Linear(Box::new(ty(TypeKind::Int))),
            TypeKind::Meta(Box::new(ty(TypeKind::Int))),
            TypeKind::Void,
            TypeKind::Identifier,
            TypeKind::RawPtr,
            TypeKind::OneOf(vec![argument(TypeKind::Int), argument(TypeKind::String)]),
            TypeKind::Error,
        ];
        for kind in unordered {
            assert_eq!(checker.ordering_of(&ty(kind.clone())), None, "{kind:?}");
        }
    }

    /// One of every `TypeKind`, each spelled with an `int` inside where it
    /// holds a type, with how a `for` loop steps through a range of it.
    fn every_type_kind() -> Vec<(TypeKind, Result<ValueStepping, NoStep>)> {
        let machine = [
            TypeKind::Int,
            TypeKind::I8,
            TypeKind::I16,
            TypeKind::I32,
            TypeKind::I64,
            TypeKind::I128,
            TypeKind::U8,
            TypeKind::U16,
            TypeKind::U32,
            TypeKind::U64,
            TypeKind::U128,
        ]
        .map(|kind| (kind, Ok(ValueStepping::Machine)));
        let float = [TypeKind::Float, TypeKind::F16, TypeKind::F32, TypeKind::F64]
            .map(|kind| (kind, Err(NoStep::Float)));
        let not_steppable = [
            TypeKind::Custom("Ranked".to_string(), None),
            TypeKind::Custom("Plain".to_string(), None),
            TypeKind::List(Box::new(argument(TypeKind::Int))),
            TypeKind::Array(
                Box::new(argument(TypeKind::Int)),
                Box::new(argument(TypeKind::Int)),
            ),
            TypeKind::Map(
                Box::new(argument(TypeKind::Int)),
                Box::new(argument(TypeKind::Int)),
            ),
            TypeKind::Set(Box::new(argument(TypeKind::Int))),
            TypeKind::Option(Box::new(ty(TypeKind::Int))),
            TypeKind::Tuple(vec![argument(TypeKind::Int), argument(TypeKind::Int)]),
            TypeKind::Result(
                Box::new(argument(TypeKind::Int)),
                Box::new(argument(TypeKind::String)),
            ),
            TypeKind::Future(Box::new(argument(TypeKind::Int))),
            TypeKind::Function(Box::new(crate::ast::types::FunctionTypeData {
                generics: None,
                params: Vec::new(),
                return_type: None,
            })),
            TypeKind::Linear(Box::new(ty(TypeKind::Int))),
            TypeKind::Meta(Box::new(ty(TypeKind::Int))),
            TypeKind::Void,
            TypeKind::Identifier,
            TypeKind::RawPtr,
            TypeKind::OneOf(vec![argument(TypeKind::Int), argument(TypeKind::String)]),
            TypeKind::Error,
        ]
        .map(|kind| (kind, Err(NoStep::NotSteppable)));
        machine
            .into_iter()
            .chain(float)
            .chain(not_steppable)
            .chain(stepped_by_declaration_or_bound())
            .collect()
    }

    /// The types whose stepping their declaration or their bound decides,
    /// with how a `for` loop steps through a range of each.
    fn stepped_by_declaration_or_bound() -> Vec<(TypeKind, Result<ValueStepping, NoStep>)> {
        let parameter = |bound: Option<&str>| {
            TypeKind::Generic(
                "T".to_string(),
                bound.map(|name| Box::new(ty(TypeKind::Custom(name.to_string(), None)))),
                crate::ast::types::TypeDeclarationKind::None,
            )
        };
        let unbounded = || Err(NoStep::UnboundedParameter("T".to_string()));
        vec![
            (TypeKind::Boolean, Err(NoStep::Boolean)),
            (TypeKind::String, Ok(ValueStepping::Method)),
            (
                TypeKind::Custom("Stepper".to_string(), None),
                Ok(ValueStepping::Method),
            ),
            (parameter(Some("Steppable")), Ok(ValueStepping::Deferred)),
            (
                TypeKind::Custom("Steppable".to_string(), None),
                Err(NoStep::TraitTyped("Steppable".to_string())),
            ),
            (parameter(Some("Comparable")), unbounded()),
            (parameter(None), unbounded()),
        ]
    }

    #[test]
    fn only_the_integers_step_by_machine() {
        let checker = checker();
        for (kind, stepping) in every_type_kind() {
            assert_eq!(
                checker.steps_by_machine(&ty(kind.clone())),
                stepping == Ok(ValueStepping::Machine),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn each_type_kind_steps_as_its_declaration_and_bound_say() {
        let checker = checker();
        for (kind, stepping) in every_type_kind() {
            assert_eq!(checker.range_step(&ty(kind.clone())), stepping, "{kind:?}");
        }
    }

    #[test]
    fn a_type_that_steps_also_orders() {
        let checker = checker();
        for (kind, _) in every_type_kind() {
            if checker.range_step(&ty(kind.clone())).is_ok() {
                assert!(
                    checker.ordering_of(&ty(kind.clone())).is_some(),
                    "{kind:?} steps but has no ordering"
                );
            }
        }
    }
}
