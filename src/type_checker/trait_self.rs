// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What `Self` names inside a trait, and where a value of the trait type
//! cannot say.
//!
//! Inside a trait, `Self` is a type parameter every trait declares without
//! writing it: the class implementing the trait, bounded by the trait at its
//! own parameters. A trait default is compiled once per implementing class, so
//! there `Self` is that class, and a class implements a trait method by
//! declaring itself wherever the trait writes `Self`. A bare trait name in a
//! trait's signature is a value of the trait type, and stays one.
//!
//! A member a trait declares, read off a receiver, reads `Self` at the
//! receiver: a class is itself, a type parameter bounded by the trait is that
//! parameter. `Self` is pinned on the declared signature, in the same
//! substitution that reads the trait's own parameters at the receiver's type
//! arguments, so a `Self` one of those arguments spells is never read again.
//! A value of the trait type does not say which class it holds, so a member
//! that needs that class — a parameter naming `Self`, or a result naming it
//! inside another type — is refused there, and such a value does not stand
//! for a type parameter bounded by a trait with such a member. A bare `Self`
//! or `Self?` result is read as the trait value, which is what the class's own
//! value converts to.

use std::collections::HashMap;

use crate::ast::bounds::Bounds;
use crate::ast::factory::make_type;
use crate::ast::operator::BinaryOp;
use crate::ast::types::{is_trait_self, Type, TypeDeclarationKind, TypeKind, SELF_TYPE_NAME};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{trait_lineage, MethodInfo, TypeDefinition};
use crate::type_checker::diagnostics::shown_type_name;
use crate::type_checker::instantiation_requirements::spells_a_type;
use crate::type_checker::TypeChecker;

/// The type `Self` names inside the trait `trait_type` (written at the
/// trait's own parameters): a parameter implementing that trait.
pub(crate) fn trait_self_type(trait_type: Type) -> Type {
    make_type(TypeKind::Generic(
        SELF_TYPE_NAME.to_string(),
        Bounds::one(TypeDeclarationKind::Implements, trait_type),
    ))
}

/// The trait whose `Self` `ty` is, when it is one.
pub(crate) fn self_trait_name(ty: &Type) -> Option<&str> {
    let TypeKind::Generic(name, bounds) = &ty.kind else {
        return None;
    };
    if name != SELF_TYPE_NAME {
        return None;
    }
    let TypeKind::Custom(trait_name, _) = &bounds.only()?.ty.kind else {
        return None;
    };
    Some(trait_name)
}

/// Whether `ty` names a trait's `Self` anywhere in it: bare, optional, as a
/// type argument, or inside a function type.
pub(crate) fn mentions_self(ty: &Type) -> bool {
    spells_a_type(&ty.kind, &is_trait_self)
}

/// Whether `ty` still spells `Self` as written, the bare keyword, anywhere in
/// it: a function type keeps its written form when it is resolved.
pub(crate) fn spells_written_self(ty: &Type) -> bool {
    spells_a_type(
        &ty.kind,
        &|kind| matches!(kind, TypeKind::Custom(name, None) if name == SELF_TYPE_NAME),
    )
}

/// Pin the trait's `Self` in `substitution` to `self_type`, so one pass of
/// `substitute_type` reads the trait's parameters and its `Self` together.
/// Every reading of a trait signature at a type goes through this or
/// [`TypeChecker::read_self_at`].
pub(crate) fn pin_self(substitution: &mut HashMap<String, Type>, self_type: Type) {
    substitution.insert(SELF_TYPE_NAME.to_string(), self_type);
}

/// Whether `ty` is `Self` or `Self?`, the results a value of the trait type
/// can stand for: the class's value converts to the trait value.
fn is_bare_or_optional_self(ty: &Type) -> bool {
    match &ty.kind {
        TypeKind::Option(inner) => is_trait_self(&inner.kind),
        kind => is_trait_self(kind),
    }
}

/// What a value of the trait type cannot supply for a member.
#[derive(Clone, Copy)]
enum SelfRefusal {
    /// A parameter names `Self`: the argument must be the receiver's class.
    Parameter,
    /// The result names `Self` inside another type, which is a different
    /// instantiation for each class.
    NestedReturn,
}

impl SelfRefusal {
    /// What the method does that needs the class, as a refusal says it.
    fn what(self) -> &'static str {
        match self {
            SelfRefusal::Parameter => "takes a `Self`",
            SelfRefusal::NestedReturn => "returns `Self` inside another type",
        }
    }
}

/// What `method`, as its trait declares it, refuses when read through a
/// value of the trait type, if anything.
fn self_refusal(method: &MethodInfo) -> Option<SelfRefusal> {
    if method.params.iter().any(|(_, ty)| mentions_self(ty)) {
        return Some(SelfRefusal::Parameter);
    }
    let result = &method.return_type;
    (mentions_self(result) && !is_bare_or_optional_self(result))
        .then_some(SelfRefusal::NestedReturn)
}

/// The help a refusal names: the values taken at one type parameter bounded
/// by the trait, so each call is compiled for the class they share.
fn bounded_parameter_help(trait_shown: &str) -> String {
    format!(
        "take the values at one type parameter bounded by the trait, as in \
         `fn f<T implements {trait_shown}>(p T, q T)`, so each call is compiled for the class \
         they share"
    )
}

impl TypeChecker {
    /// `ty`, a type a trait declared, with its `Self` read as `self_type`.
    /// A type that does not name `Self` is returned as it is.
    pub(crate) fn read_self_at(&self, ty: &Type, self_type: &Type) -> Type {
        if !mentions_self(ty) {
            return ty.clone();
        }
        let mut substitution = HashMap::with_capacity(1);
        pin_self(&mut substitution, self_type.clone());
        self.substitute_type(ty, &substitution)
    }

    /// `pins`, what the receiver's type arguments bind the declaring trait's
    /// parameters to, with the trait's `Self` pinned to `receiver`; `None`
    /// once a refusal of `method` through a value of the trait type is
    /// reported.
    pub(crate) fn pins_at_receiver(
        &mut self,
        mut pins: HashMap<String, Type>,
        (method_name, method): (&str, &MethodInfo),
        receiver: &Type,
        span: Span,
    ) -> Option<HashMap<String, Type>> {
        if let Some(trait_name) = self.trait_value_name(receiver) {
            if let Some(refusal) = self_refusal(method) {
                let trait_name = trait_name.to_string();
                self.report_self_through_trait_value(refusal, method_name, &trait_name, span);
                return None;
            }
        }
        pin_self(&mut pins, receiver.clone());
        Some(pins)
    }

    /// The trait `ty` is a value of, when it is one, an optional of one
    /// included.
    pub(crate) fn trait_value_name<'t>(&self, ty: &'t Type) -> Option<&'t str> {
        let ty = if let TypeKind::Option(inner) = &ty.kind {
            inner
        } else {
            ty
        };
        let TypeKind::Custom(name, _) = &ty.kind else {
            return None;
        };
        matches!(
            self.type_table.global_type_definitions.get(name),
            Some(TypeDefinition::Trait(_))
        )
        .then_some(name.as_str())
    }

    /// The trait and method refusing `op` applied to these operands: an
    /// operand is a value of a trait type whose method answering `op` takes
    /// a `Self`, which the value cannot say the class of. Comparing such
    /// values any other way would compare where they live rather than what
    /// they hold.
    pub(crate) fn operator_self_refusal(
        &self,
        (left, op, right): (&Type, &BinaryOp, &Type),
    ) -> Option<(String, &'static str)> {
        let method = crate::ast::implicit_methods::operator_method_name(op)?;
        [left, right].into_iter().find_map(|operand| {
            let trait_name = self.trait_value_name(operand)?;
            let takes_self = self
                .trait_method(trait_name, method)?
                .params
                .iter()
                .any(|(_, ty)| mentions_self(ty));
            takes_self.then(|| (trait_name.to_string(), method))
        })
    }

    /// Refuse `op` applied to these operands when
    /// [`Self::operator_self_refusal`] does; whether it refused.
    pub(crate) fn refuses_operator_on_trait_value(
        &mut self,
        (left, op, right): (&Type, &BinaryOp, &Type),
        span: Span,
    ) -> bool {
        let Some((trait_name, method)) = self.operator_self_refusal((left, op, right)) else {
            return false;
        };
        self.report_operator_through_trait_value((&trait_name, op, method), span);
        true
    }

    /// Report `op`, answered by `method` of `trait_name`, refused on a value
    /// of the trait type.
    pub(crate) fn report_operator_through_trait_value(
        &mut self,
        (trait_name, op, method): (&str, &BinaryOp, &str),
        span: Span,
    ) {
        let shown = shown_type_name(trait_name);
        let operator = crate::ast::formatter::helpers::binary_operator(*op);
        self.report_error_with_help(
            DiagnosticCode::TypSelfThroughTraitValue,
            format!(
                "`{operator}` runs `{method}`, which takes a `Self`, and a value of the trait \
                 `{shown}` does not say which class that is"
            ),
            span,
            bounded_parameter_help(shown),
        );
    }

    /// The method `method` the trait `trait_name` declares, or one of the
    /// traits it extends does.
    fn trait_method(&self, trait_name: &str, method: &str) -> Option<&MethodInfo> {
        trait_lineage(&self.type_table.global_type_definitions, trait_name)
            .find_map(|(_, trait_def)| trait_def.methods.get(method))
    }

    /// The method, and what it needs, that keeps `argument`, a value of a
    /// trait type, from meeting the bound `implements constraint`: a method
    /// of the bound's trait, or of one it extends, that a value of a trait
    /// type cannot be handed. A type parameter bounded that way is called
    /// with two of its values as one class, which two values of a trait
    /// type need not be. `None` when the argument is no trait value or the
    /// bound's trait has no such method.
    fn bound_self_refusal(
        &self,
        argument: &Type,
        constraint: &Type,
    ) -> Option<(&str, SelfRefusal)> {
        let TypeKind::Custom(_, _) = &argument.kind else {
            return None;
        };
        self.trait_value_name(argument)?;
        let TypeKind::Custom(bound_trait, _) = &constraint.kind else {
            return None;
        };
        trait_lineage(&self.type_table.global_type_definitions, bound_trait)
            .flat_map(|(_, trait_def)| trait_def.methods.iter())
            .filter_map(|(name, method)| Some((name.as_str(), self_refusal(method)?)))
            .min_by_key(|(name, _)| *name)
    }

    /// Whether `argument` misses the bound `kind constraint` because it is a
    /// value of a trait type and the bound needs the class (see
    /// [`Self::bound_self_refusal`]).
    pub(crate) fn misses_bound_for_its_class(
        &self,
        argument: &Type,
        constraint: &Type,
        kind: &TypeDeclarationKind,
    ) -> bool {
        matches!(kind, TypeDeclarationKind::Implements)
            && self.bound_self_refusal(argument, constraint).is_some()
    }

    /// Report `argument` missing the bound `implements constraint` for the
    /// reason [`Self::misses_bound_for_its_class`] gives; whether it did.
    pub(crate) fn reports_bound_needing_the_class(
        &mut self,
        argument: &Type,
        constraint: &Type,
        span: Span,
    ) -> bool {
        let Some((method, refusal)) = self.bound_self_refusal(argument, constraint) else {
            return false;
        };
        let method = method.to_string();
        let Some(trait_name) = self.trait_value_name(argument).map(str::to_string) else {
            return false;
        };
        let shown = shown_type_name(&trait_name);
        let TypeKind::Custom(bound, _) = &constraint.kind else {
            return false;
        };
        let bound = shown_type_name(bound);
        self.report_error_with_help(
            DiagnosticCode::TypSelfThroughTraitValue,
            format!(
                "a value of the trait `{shown}` cannot stand for a type parameter bounded by \
                 `{bound}`: the method `{method}` {}, and the value does not say which class \
                 that is",
                refusal.what()
            ),
            span,
            format!(
                "hand the values over at their class, or take them at a type parameter of the \
                 caller's own, as in `fn f<T implements {bound}>(p T, q T)`"
            ),
        );
        true
    }

    fn report_self_through_trait_value(
        &mut self,
        refusal: SelfRefusal,
        method: &str,
        trait_name: &str,
        span: Span,
    ) {
        let shown = shown_type_name(trait_name);
        self.report_error_with_help(
            DiagnosticCode::TypSelfThroughTraitValue,
            format!(
                "the method `{method}` {}, and a value of the trait `{shown}` does not say \
                 which class that is",
                refusal.what()
            ),
            span,
            bounded_parameter_help(shown),
        );
    }
}
