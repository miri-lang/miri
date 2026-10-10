// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The methods a program runs without writing a call to them: the ones
//! building and releasing an instance runs, the ones a container's runtime
//! thunk asks of its elements, and the ones an operator dispatches to.
//!
//! This is part of the language's contract, so it lives beside the syntax.
//! The type checker answers a generic class's requirements wherever one of
//! these is used, and MIR lowering and code generation name the bodies they
//! run; each reads the names from here, so a method one of them runs is a
//! method the other has checked.

use crate::ast::operator::BinaryOp;
pub use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{
    Type, TypeKind, ADDING_TRAIT_NAME, EQUALITY_TRAIT_NAME, ORDERING_TRAIT_NAME,
    REPEATING_TRAIT_NAME,
};
pub use crate::ast::types::{
    CLONE_METHOD_NAME, EQUALS_METHOD_NAME, HASH_METHOD_NAME, ORDERING_METHOD_NAME,
};

/// The method a construction runs to initialise an instance.
pub const INIT_METHOD_NAME: &str = "init";

/// The methods every instance runs by being built and released.
pub const CONSTRUCTION_METHOD_NAMES: [&str; 2] = [INIT_METHOD_NAME, DROP_HOOK_NAME];

/// The methods a container's runtime thunk asks of two class elements: the
/// ordering it sorts by and the equality it matches by.
pub const ELEMENT_METHOD_NAMES: [&str; 2] = [ORDERING_METHOD_NAME, EQUALS_METHOD_NAME];

/// The methods a runtime thunk calls on a class instance, besides its drop
/// hook: a container copies, orders, matches and hashes its elements through
/// them.
pub const THUNK_METHOD_NAMES: [&str; 4] = {
    let [ordering, equals] = ELEMENT_METHOD_NAMES;
    [CLONE_METHOD_NAME, ordering, equals, HASH_METHOD_NAME]
};

/// The method `+` dispatches to on a type that defines it, the string type
/// included.
pub const CONCAT_METHOD_NAME: &str = "concat";

/// The method `*` dispatches to on a type that defines it.
pub const REPEAT_METHOD_NAME: &str = "repeat";

/// Every method an operator dispatches to on a type that defines it.
pub const OPERATOR_METHOD_NAMES: [&str; 4] = [
    CONCAT_METHOD_NAME,
    REPEAT_METHOD_NAME,
    EQUALS_METHOD_NAME,
    ORDERING_METHOD_NAME,
];

/// How an operator reads the value the method it dispatches to returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MethodResultReading {
    /// The method's result is the operator's result (`+`, `*`, `==`).
    AsReturned,
    /// The operator is the negation of the method's boolean result (`!=`).
    Negated,
    /// The method's `int` result is below zero (`<`).
    BelowZero,
    /// The method's `int` result is at most zero (`<=`).
    AtMostZero,
    /// The method's `int` result is above zero (`>`).
    AboveZero,
    /// The method's `int` result is at least zero (`>=`).
    AtLeastZero,
}

/// The method an operator dispatches to, the trait that declares it, and how
/// the operator reads its result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorMethod {
    pub name: &'static str,
    pub trait_name: &'static str,
    pub reading: MethodResultReading,
}

/// The method `op` dispatches to when its receiver's type defines it, or
/// `None` for an operator no method answers.
///
/// One `compare` answers all four ordering operators and one `equals` answers
/// both equality operators.
pub fn operator_method(op: &BinaryOp) -> Option<OperatorMethod> {
    let (name, trait_name, reading) = match op {
        BinaryOp::Add => (
            CONCAT_METHOD_NAME,
            ADDING_TRAIT_NAME,
            MethodResultReading::AsReturned,
        ),
        BinaryOp::Mul => (
            REPEAT_METHOD_NAME,
            REPEATING_TRAIT_NAME,
            MethodResultReading::AsReturned,
        ),
        BinaryOp::Equal => (
            EQUALS_METHOD_NAME,
            EQUALITY_TRAIT_NAME,
            MethodResultReading::AsReturned,
        ),
        BinaryOp::NotEqual => (
            EQUALS_METHOD_NAME,
            EQUALITY_TRAIT_NAME,
            MethodResultReading::Negated,
        ),
        BinaryOp::LessThan => (
            ORDERING_METHOD_NAME,
            ORDERING_TRAIT_NAME,
            MethodResultReading::BelowZero,
        ),
        BinaryOp::LessThanEqual => (
            ORDERING_METHOD_NAME,
            ORDERING_TRAIT_NAME,
            MethodResultReading::AtMostZero,
        ),
        BinaryOp::GreaterThan => (
            ORDERING_METHOD_NAME,
            ORDERING_TRAIT_NAME,
            MethodResultReading::AboveZero,
        ),
        BinaryOp::GreaterThanEqual => (
            ORDERING_METHOD_NAME,
            ORDERING_TRAIT_NAME,
            MethodResultReading::AtLeastZero,
        ),
        BinaryOp::Sub
        | BinaryOp::Div
        | BinaryOp::Mod
        | BinaryOp::BitwiseOr
        | BinaryOp::BitwiseAnd
        | BinaryOp::BitwiseXor
        | BinaryOp::Not
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Range
        | BinaryOp::In
        | BinaryOp::NullCoalesce => return None,
    };
    Some(OperatorMethod {
        name,
        trait_name,
        reading,
    })
}

/// The trait whose method answers `op`, or `None` for an operator no method
/// answers.
pub fn operator_trait_name(op: &BinaryOp) -> Option<&'static str> {
    operator_method(op).map(|method| method.trait_name)
}

/// The name of the method `op` dispatches to, or `None` for an operator no
/// method answers.
pub fn operator_method_name(op: &BinaryOp) -> Option<&'static str> {
    operator_method(op).map(|method| method.name)
}

/// One operator for each method an operator dispatches to: `+`, `*`, `==`
/// and `<`, the operator that names its method's trait.
const NAMING_OPERATORS: [BinaryOp; 4] = [
    BinaryOp::Add,
    BinaryOp::Mul,
    BinaryOp::Equal,
    BinaryOp::LessThan,
];

/// The operator dispatching to the method `trait_name` declares, and that
/// method, when the trait is one an operator dispatches to.
///
/// A type the language defines the operator on answers the method through
/// the operator, without declaring either, so `int` meets `Comparable`.
pub fn operator_naming_trait(trait_name: &str) -> Option<(BinaryOp, OperatorMethod)> {
    NAMING_OPERATORS.into_iter().find_map(|op| {
        operator_method(&op)
            .filter(|method| method.trait_name == trait_name)
            .map(|method| (op, method))
    })
}

/// The operator dispatching to the method called `method_name`, and that
/// method, when an operator dispatches to one of that name.
pub fn operator_naming_method(method_name: &str) -> Option<(BinaryOp, OperatorMethod)> {
    NAMING_OPERATORS.into_iter().find_map(|op| {
        operator_method(&op)
            .filter(|method| method.name == method_name)
            .map(|method| (op, method))
    })
}

/// The type whose method `op` runs when applied to a left operand of type
/// `left`.
///
/// An operator dispatches on its left operand. Equality is also lifted
/// through an optional: two optionals are equal when both are empty or both
/// hold equal payloads, which compares the payloads with their own `equals`,
/// so an equality on `T?` runs `T`'s method.
pub fn operator_receiver<'t>(op: &BinaryOp, left: &'t Type) -> &'t Type {
    let lifts_through_optionals = matches!(op, BinaryOp::Equal | BinaryOp::NotEqual);
    let mut receiver = left;
    while let (true, TypeKind::Option(payload)) = (lifts_through_optionals, &receiver.kind) {
        receiver = payload;
    }
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::syntax::Span;

    #[test]
    fn every_operator_method_is_named_by_some_operator() {
        let named: Vec<&str> = NAMING_OPERATORS
            .iter()
            .filter_map(operator_method_name)
            .collect();
        assert_eq!(named, OPERATOR_METHOD_NAMES);
    }

    #[test]
    fn an_operator_trait_and_its_method_name_the_same_operator() {
        for trait_name in [
            ADDING_TRAIT_NAME,
            REPEATING_TRAIT_NAME,
            EQUALITY_TRAIT_NAME,
            ORDERING_TRAIT_NAME,
        ] {
            let naming = operator_naming_trait(trait_name);
            assert!(naming.is_some(), "{trait_name} names no operator");
            assert_eq!(
                naming.and_then(|(_, method)| operator_naming_method(method.name)),
                naming
            );
        }
        assert_eq!(operator_naming_trait("Hashable"), None);
        assert_eq!(operator_naming_method("hash"), None);
    }

    #[test]
    fn an_operator_with_no_method_names_none() {
        assert_eq!(operator_method_name(&BinaryOp::Sub), None);
        assert_eq!(operator_method_name(&BinaryOp::And), None);
    }

    #[test]
    fn inequality_reads_the_negated_equals() {
        assert_eq!(
            operator_method(&BinaryOp::NotEqual),
            Some(OperatorMethod {
                name: EQUALS_METHOD_NAME,
                trait_name: EQUALITY_TRAIT_NAME,
                reading: MethodResultReading::Negated
            })
        );
    }

    /// Each operator's trait comes from the same table as its method.
    #[test]
    fn an_operator_names_the_trait_that_declares_its_method() {
        assert_eq!(operator_trait_name(&BinaryOp::Add), Some(ADDING_TRAIT_NAME));
        assert_eq!(
            operator_trait_name(&BinaryOp::Mul),
            Some(REPEATING_TRAIT_NAME)
        );
        assert_eq!(
            operator_trait_name(&BinaryOp::LessThan),
            Some(ORDERING_TRAIT_NAME)
        );
        assert_eq!(operator_trait_name(&BinaryOp::Sub), None);
    }

    fn optional(inner: Type) -> Type {
        Type::new(TypeKind::Option(Box::new(inner)), Span::new(0, 0))
    }

    #[test]
    fn equality_on_an_optional_runs_the_payloads_method() {
        let named = Type::new(TypeKind::Custom("Box".into(), None), Span::new(0, 0));
        let nested = optional(optional(named.clone()));
        assert_eq!(
            operator_receiver(&BinaryOp::Equal, &nested).kind,
            named.kind
        );
    }

    #[test]
    fn ordering_on_an_optional_is_not_lifted() {
        let named = Type::new(TypeKind::Custom("Box".into(), None), Span::new(0, 0));
        let wrapped = optional(named);
        assert_eq!(
            operator_receiver(&BinaryOp::LessThan, &wrapped).kind,
            wrapped.kind
        );
    }
}
