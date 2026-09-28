// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The methods a program runs without writing a call to them: the ones
//! building and releasing an instance runs, the ones a container's runtime
//! thunk asks of its elements, and the ones an operator dispatches to.
//!
//! The type checker answers a generic class's requirements wherever one of
//! these is used, and MIR lowering and code generation name the bodies they
//! run; each reads the names from here, so a method one of them runs is a
//! method the other has checked.

use crate::ast::operator::BinaryOp;
pub use crate::ast::statement::DROP_HOOK_NAME;
pub use crate::ast::types::{CLONE_METHOD_NAME, EQUALS_METHOD_NAME, ORDERING_METHOD_NAME};

/// The method a construction runs to initialise an instance.
pub const INIT_METHOD_NAME: &str = "init";

/// The methods every instance runs by being built and released.
pub const CONSTRUCTION_METHOD_NAMES: [&str; 2] = [INIT_METHOD_NAME, DROP_HOOK_NAME];

/// The methods a container's runtime thunk asks of two class elements: the
/// ordering it sorts by and the equality it matches by.
pub const ELEMENT_METHOD_NAMES: [&str; 2] = [ORDERING_METHOD_NAME, EQUALS_METHOD_NAME];

/// The methods a runtime thunk calls on a class instance, besides its drop
/// hook.
pub const THUNK_METHOD_NAMES: [&str; 3] = {
    let [ordering, equals] = ELEMENT_METHOD_NAMES;
    [CLONE_METHOD_NAME, ordering, equals]
};

/// The method `+` dispatches to on a type that defines it.
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

/// The method `op` dispatches to when its left operand's type defines it, or
/// `None` for an operator no method answers.
///
/// One `compare` answers all four ordering operators and one `equals` answers
/// both equality operators; how the operator reads the method's result is the
/// lowering's to say.
pub fn operator_method_name(op: &BinaryOp) -> Option<&'static str> {
    match op {
        BinaryOp::Add => Some(CONCAT_METHOD_NAME),
        BinaryOp::Mul => Some(REPEAT_METHOD_NAME),
        BinaryOp::Equal | BinaryOp::NotEqual => Some(EQUALS_METHOD_NAME),
        BinaryOp::LessThan
        | BinaryOp::LessThanEqual
        | BinaryOp::GreaterThan
        | BinaryOp::GreaterThanEqual => Some(ORDERING_METHOD_NAME),
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
        | BinaryOp::NullCoalesce => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_operator_method_is_named_by_some_operator() {
        let all = [
            BinaryOp::Add,
            BinaryOp::Mul,
            BinaryOp::Equal,
            BinaryOp::LessThan,
        ];
        let named: Vec<&str> = all.iter().filter_map(operator_method_name).collect();
        assert_eq!(named, OPERATOR_METHOD_NAMES);
    }

    #[test]
    fn an_operator_with_no_method_names_none() {
        assert_eq!(operator_method_name(&BinaryOp::Sub), None);
        assert_eq!(operator_method_name(&BinaryOp::And), None);
    }
}
