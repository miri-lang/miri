// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The traits a type meets without declaring them.
//!
//! A class says which traits it implements, and meets those. A value of any
//! other type — a number, a `bool`, a struct, an enum, a tuple, an optional —
//! has no `implements` clause to write, yet the language already gives it the
//! operators those traits stand for: numbers order and add, a struct compares
//! field by field. Such a type meets the trait an operator dispatches to
//! whenever that operator applies to it, and answers the trait's method
//! through the operator: `a.compare(b)` orders as `<` does, `a.equals(b)` is
//! `==`, `a.concat(b)` is `+` and `a.repeat(n)` is `*`. It meets `Hashable`
//! wherever `==` compares it and a hash is derived from the same parts, and
//! `Accelerable` wherever its values can live on a device.
//!
//! Whether an operator applies is answered by replaying the checker's own
//! judgment of that operator, so a type meets a trait exactly when the
//! operator the trait stands for would type-check on it. Only the traits an
//! operator names are met this way: a trait of the program's own is met by
//! declaring it, never by a method that happens to share a name.

use crate::ast::factory::{make_type, type_expr_non_null};
use crate::ast::implicit_methods::{operator_naming_method, operator_naming_trait};
use crate::ast::operator::BinaryOp;
use crate::ast::types::{
    FunctionTypeData, Type, TypeKind, ACCELERABLE_TRAIT_NAME, EQUALITY_TRAIT_NAME,
    HASHING_TRAIT_NAME,
};
use crate::ast::Parameter;
use crate::type_checker::context::{Context, MethodInfo, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

impl TypeChecker {
    /// Whether `ty` meets the trait `trait_name` without declaring it.
    pub(crate) fn derives_trait(&self, ty: &Type, trait_name: &str, context: &Context) -> bool {
        if trait_name == ACCELERABLE_TRAIT_NAME {
            return crate::type_checker::utils::is_accelerable(&ty.kind, self.type_definitions());
        }
        if !self.conforms_by_value(ty) {
            return false;
        }
        if trait_name == HASHING_TRAIT_NAME {
            return self.derives_trait(ty, EQUALITY_TRAIT_NAME, context)
                && self.type_lacking_hash(ty).is_none();
        }
        operator_naming_trait(trait_name).is_some_and(|(op, method)| {
            self.derived_method_type(ty, trait_name, method.name)
                .is_some_and(|declared| self.operator_answers(ty, &op, &declared, context))
        })
    }

    /// The type of the method `method_name` a value of `ty` answers through
    /// the operator dispatching to it, or `None` when it answers none.
    pub(crate) fn derived_operator_member(
        &self,
        ty: &Type,
        method_name: &str,
        context: &Context,
    ) -> Option<Type> {
        let (_, method) = operator_naming_method(method_name)?;
        if !self.derives_trait(ty, method.trait_name, context) {
            return None;
        }
        let declared = self.derived_method_type(ty, method.trait_name, method.name)?;
        Some(function_type(&declared))
    }

    /// Whether a call of the operator method `method_name` on a value of `ty`
    /// is answered by the operator rather than dispatched to a method: `ty`
    /// is not a class and declares no method of that name.
    pub(crate) fn answers_operator_methods_by_operator(
        &self,
        ty: &Type,
        method_name: &str,
    ) -> bool {
        self.conforms_by_value(ty) && !self.declares_own_method(ty, method_name)
    }

    /// Whether `ty` meets traits by what the language defines on it rather
    /// than by declaring them. A class, the string and the collections are
    /// stdlib or program classes whose methods are the ones they declare; a
    /// type parameter meets what its bound says.
    fn conforms_by_value(&self, ty: &Type) -> bool {
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
            | TypeKind::Boolean
            | TypeKind::Tuple(_)
            | TypeKind::Option(_)
            | TypeKind::Result(_, _) => true,
            TypeKind::Custom(name, _) => matches!(
                self.type_definitions().get(name),
                Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_))
            ),
            TypeKind::String
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
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

    /// The method `method_name` the trait `trait_name` declares, read at a
    /// value of `ty`: each `Self` the trait wrote is `ty`. `None` when the
    /// trait is not loaded, or when `ty` declares a method of that name of
    /// its own, which is called like any other.
    fn derived_method_type(
        &self,
        ty: &Type,
        trait_name: &str,
        method_name: &str,
    ) -> Option<MethodInfo> {
        if self.declares_own_method(ty, method_name) {
            return None;
        }
        let Some(TypeDefinition::Trait(trait_def)) = self.type_definitions().get(trait_name) else {
            return None;
        };
        let declared = trait_def.methods.get(method_name)?;
        let as_self = HashMap::from([(trait_name.to_string(), ty.clone())]);
        let mut method = declared.clone();
        for (_, param) in &mut method.params {
            *param = self.substitute_type(param, &as_self);
        }
        method.return_type = self.substitute_type(&declared.return_type, &as_self);
        Some(method)
    }

    /// Whether `op` applied to a value of `ty` and the method's argument
    /// gives what the method returns. An ordering or equality answers a
    /// method whose result the operator reads; `+` and `*` must give a value
    /// of the method's own type, so `i8 * int`, which widens, answers no
    /// `repeat` returning an `i8`.
    fn operator_answers(
        &self,
        ty: &Type,
        op: &BinaryOp,
        method: &MethodInfo,
        context: &Context,
    ) -> bool {
        let [(_, argument)] = method.params.as_slice() else {
            return false;
        };
        let Ok(result) = self.replay_method_operator(ty, op, argument, context) else {
            return false;
        };
        match op {
            BinaryOp::Add | BinaryOp::Mul => result.kind == method.return_type.kind,
            BinaryOp::Equal | BinaryOp::LessThan => true,
            BinaryOp::Sub
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::NotEqual
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual
            | BinaryOp::BitwiseOr
            | BinaryOp::BitwiseAnd
            | BinaryOp::BitwiseXor
            | BinaryOp::Not
            | BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Range
            | BinaryOp::In
            | BinaryOp::NullCoalesce => false,
        }
    }

    /// The checker's judgment of `left op right` for an operator that
    /// dispatches to a method, by shared reference so a bound can ask it.
    fn replay_method_operator(
        &self,
        left: &Type,
        op: &BinaryOp,
        right: &Type,
        context: &Context,
    ) -> Result<Type, String> {
        match op {
            BinaryOp::Add | BinaryOp::Mul => self.check_arithmetic_op(left, op, right, context),
            BinaryOp::Equal => self.check_equality_op(left, right, context),
            BinaryOp::LessThan => self.check_ordering_op(left, op, right, context),
            BinaryOp::Sub
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::NotEqual
            | BinaryOp::LessThanEqual
            | BinaryOp::GreaterThan
            | BinaryOp::GreaterThanEqual
            | BinaryOp::BitwiseOr
            | BinaryOp::BitwiseAnd
            | BinaryOp::BitwiseXor
            | BinaryOp::Not
            | BinaryOp::And
            | BinaryOp::Or
            | BinaryOp::Range
            | BinaryOp::In
            | BinaryOp::NullCoalesce => Err(format!("no method answers '{op:?}'")),
        }
    }

    /// Whether `ty` names a struct or enum declaring `method_name` itself.
    fn declares_own_method(&self, ty: &Type, method_name: &str) -> bool {
        if let TypeKind::Custom(name, _) = &ty.kind {
            return self.type_declares_method(name, method_name);
        }
        false
    }
}

/// The function type of calling `method` on a receiver.
fn function_type(method: &MethodInfo) -> Type {
    let params = method
        .params
        .iter()
        .map(|(name, ty)| Parameter {
            name_span: Default::default(),
            name: name.clone(),
            typ: Box::new(type_expr_non_null(ty.clone())),
            guard: None,
            default_value: None,
            is_out: false,
            residency: None,
        })
        .collect();
    make_type(TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params,
        return_type: Some(Box::new(type_expr_non_null(method.return_type.clone()))),
    })))
}
