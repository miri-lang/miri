// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement type checking for the type checker.
//!
//! This module implements type checking for all statement kinds in Miri.
//! The main entry point is [`TypeChecker::check_statement`], which validates
//! statements and registers type information in the context.
//!
//! # Supported Statements
//!
//! ## Declarations
//! - Variable declarations: `let x = 1`, `var y: int = 2`
//! - Function declarations with generics and return type validation
//! - Struct, enum, class, and trait definitions
//! - Type aliases
//!
//! ## Control Flow
//! - If/else statements with condition type checking
//! - While loops (including forever loops)
//! - For loops with iterator type inference
//! - Match statements with exhaustiveness checking
//! - Return statements with type compatibility validation
//!
//! ## Expressions
//! - Expression statements (side effects)
//! - Assignment validation
//!
//! ## Type Definitions
//! - Structs with fields and generic parameters
//! - Enums with variants and associated values
//! - Classes with fields, methods, and inheritance
//! - Traits with method signatures
//!
//! # Return Type Analysis
//!
//! The module includes return status analysis (`check_returns`) to determine:
//! - Whether all code paths return a value
//! - Implicit vs explicit returns
//! - Return type compatibility

use crate::ast::factory::make_type;
use crate::ast::gpu_wire::buffer_element_wire;
use crate::ast::statement::BindingResidency;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::utils::{
    is_accelerable, is_gpu_compatible, resolve_element_type_kind, type_mentions_f16,
};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// Whether a written type's arguments name the same parameters the
    /// initializer's do.
    ///
    /// Inside `fn f<T>`, `Tagged<T>` and `Tagged<int>` are different types: the
    /// body is compiled for whatever `T` turns out to be, and nothing makes
    /// that an `int`. The general rule treats an unconstrained parameter as
    /// matching anything, which is right where the parameter is the whole
    /// declared type — `var x T = a` takes whatever the body is instantiated
    /// at — and wrong one level in, where the two spellings have to agree.
    ///
    /// Only a parameter **the enclosing body declares** is held to this. A
    /// parameter name that resolves to nothing in scope is an inference slot
    /// the declaration is there to fill: `let r Result<int, bool> =
    /// Result.Ok(42)` infers `Result<int, E>`, because `Ok` pins one side only
    /// and the annotation is what settles the other.
    ///
    /// Only argument positions are compared, and only when both sides spell the
    /// same type with the same arity; anything else is left to the general
    /// rule, which already answers it.
    fn type_arguments_name_the_same_parameters(
        &self,
        declared: &Type,
        inferred: &Type,
        context: &Context,
    ) -> bool {
        let (
            TypeKind::Custom(declared_name, Some(declared_args)),
            TypeKind::Custom(inferred_name, Some(inferred_args)),
        ) = (&declared.kind, &inferred.kind)
        else {
            return true;
        };
        if declared_name != inferred_name || declared_args.len() != inferred_args.len() {
            return true;
        }
        declared_args
            .iter()
            .zip(inferred_args)
            .all(|(declared_arg, inferred_arg)| {
                Self::argument_parameters_agree(declared_arg, inferred_arg, context)
            })
    }

    /// Whether one written type argument may stand where the other is declared.
    ///
    /// A parameter the body declares agrees only with the same parameter by
    /// name. Anything else is left to the general rule.
    fn argument_parameters_agree(
        declared: &Expression,
        inferred: &Expression,
        context: &Context,
    ) -> bool {
        let Some(declared_param) = Self::declared_parameter_in_scope(declared, context) else {
            return true;
        };
        Self::written_parameter_name(inferred) == Some(declared_param)
    }

    /// The name of the enclosing body's type parameter a written argument is,
    /// or `None` when it names anything else.
    fn declared_parameter_in_scope<'e>(arg: &'e Expression, context: &Context) -> Option<&'e str> {
        let name = Self::written_parameter_name(arg)?;
        matches!(
            context.resolve_type_definition(name),
            Some(crate::type_checker::context::TypeDefinition::Generic(_))
        )
        .then_some(name)
    }

    /// The name of the type parameter a written type argument is, or `None`
    /// when it names something else.
    fn written_parameter_name(arg: &Expression) -> Option<&str> {
        let ExpressionKind::Type(ty, _) = &arg.node else {
            return None;
        };
        let TypeKind::Generic(name, _, _) = &ty.kind else {
            return None;
        };
        Some(name.as_str())
    }

    pub(crate) fn check_variable_declaration(
        &mut self,
        decls: &[VariableDeclaration],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        // A statement binding one name owns its `let` keyword outright, so that
        // keyword can be rewritten for that binding alone. A statement binding
        // several shares one keyword between them and records none.
        let keyword_start = match decls {
            [_] => Some(span.start),
            _ => None,
        };
        for decl in decls {
            if decl.is_shared {
                self.validate_shared_variable(decl, context, span);
            }
            self.register_variable_decl(decl, visibility, context, span, keyword_start);
        }
    }

    fn validate_shared_variable(
        &mut self,
        decl: &VariableDeclaration,
        context: &mut Context,
        span: Span,
    ) {
        if !context.in_gpu_function {
            self.report_error(
                DiagnosticCode::TypSharedVariable,
                "Shared variables can only be declared inside 'gpu' functions".to_string(),
                span,
            );
        }

        if let Some(typ_expr) = &decl.typ {
            let resolved_type = self.resolve_type_expression(typ_expr, context);
            let is_array = matches!(&resolved_type.kind, TypeKind::Array(_, _))
                || matches!(&resolved_type.kind, TypeKind::Custom(n, Some(_)) if BuiltinCollectionKind::from_name(n) == Some(BuiltinCollectionKind::Array));
            if !is_array {
                self.report_error(
                    DiagnosticCode::TypSharedVariable,
                    format!(
                        "Shared variable '{}' must be an array, got {}",
                        decl.name, resolved_type
                    ),
                    span,
                );
            }
        } else {
            self.report_error(
                DiagnosticCode::TypSharedVariable,
                format!("Shared variable '{}' must have an explicit type", decl.name),
                span,
            );
        }

        if decl.initializer.is_some() {
            self.report_error(
                DiagnosticCode::TypSharedVariable,
                format!("Shared variable '{}' cannot have an initializer", decl.name),
                span,
            );
        }
    }

    pub(crate) fn register_variable_decl(
        &mut self,
        decl: &VariableDeclaration,
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
        keyword_start: Option<usize>,
    ) {
        let inferred_type = self.determine_variable_type(decl, context, span);
        self.check_gpu_variable_type(&decl.name, &inferred_type, context, span);
        if let (true, Some(type_expr)) = (context.in_gpu_function, &decl.typ) {
            self.reject_device_wide_integer(&inferred_type.kind, type_expr.span);
        }
        self.check_gpu_residency_type(decl, &inferred_type, context, span);
        self.check_host_f16(decl, &inferred_type, context, span);
        let is_mutable = matches!(
            decl.declaration_type,
            VariableDeclarationType::Mutable | VariableDeclarationType::Unmarked
        );
        let is_constant = matches!(decl.declaration_type, VariableDeclarationType::Constant);

        // A binding that cannot be reassigned holds one value for its whole
        // lifetime, so an initializer known at compile time is known at every
        // use site too. `var` is excluded: a later assignment would invalidate
        // the recorded value.
        let const_value = if is_mutable {
            None
        } else {
            decl.initializer
                .as_ref()
                .and_then(|init| Self::constant_literal(init, &inferred_type.kind, context))
        };

        // A top-level binding is shadow-checked once, in the declaration-collection
        // pass. When the body pass revisits the same declaration it is a
        // re-registration of that hoisted binding, not a redeclaration, so the
        // shadow check is skipped to avoid a spurious "cannot shadow" against itself.
        let is_hoisted_top_level =
            context.scopes.len() == 1 && self.hoisted_top_level.contains(&decl.name);
        if !is_hoisted_top_level {
            self.check_shadowing(&decl.name, is_mutable, is_constant, context, span);
        }

        let mut info = SymbolInfo::new(
            inferred_type,
            is_mutable,
            is_constant,
            visibility.clone(),
            self.modules.current_module.clone(),
            const_value,
        );
        info.residency = decl.residency;
        // Only an immutable binding can be repaired into a mutable one. A
        // constant is a different declaration form, so rewriting `let` there
        // would not compile.
        if matches!(decl.declaration_type, VariableDeclarationType::Immutable) {
            info.declaration_keyword_start = keyword_start;
        }

        // Track whether this binding is at module scope (public API surface).
        let is_at_module_scope = context.scopes.len() == 1;
        info.module_scope = is_at_module_scope;

        if is_at_module_scope {
            self.type_table
                .global_scope
                .insert(decl.name.clone(), info.clone());
        }
        context.define(decl.name.clone(), info);
    }

    /// The compile-time value of an immutable binding's initializer, shaped to
    /// the binding's type `kind`.
    ///
    /// An integer expression that folds becomes an integer of that type, or a
    /// float when the binding is a float. Otherwise the initializer must be a
    /// float, boolean or string literal, a negated float, or the name of a
    /// binding that already has a value. Anything else has no value until it
    /// runs, and yields `None`.
    fn constant_literal(init: &Expression, kind: &TypeKind, context: &Context) -> Option<Literal> {
        if let Some(value) = Self::try_eval_const_int_with_context(init, context) {
            return integer_constant(kind, value);
        }
        match &init.node {
            ExpressionKind::Literal(
                literal @ (Literal::Float(_) | Literal::Boolean(_) | Literal::String(_)),
            ) => Some(literal.clone()),
            ExpressionKind::Unary(UnaryOp::Negate, inner) => {
                match Self::constant_literal(inner, kind, context)? {
                    Literal::Float(float) => Some(Literal::Float(negated_float(float))),
                    Literal::Integer(_)
                    | Literal::String(_)
                    | Literal::Boolean(_)
                    | Literal::Identifier(_)
                    | Literal::Regex(_)
                    | Literal::None => None,
                }
            }
            ExpressionKind::Identifier(name, _) => {
                let info = context.resolve_info(name)?;
                if info.mutable && !info.is_constant {
                    return None;
                }
                info.value.clone()
            }
            _ => None,
        }
    }

    fn check_gpu_variable_type(
        &mut self,
        name: &str,
        inferred_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if !context.in_gpu_function {
            return;
        }
        if matches!(inferred_type.kind, TypeKind::Error) {
            return;
        }
        if is_gpu_compatible(&inferred_type.kind) {
            return;
        }
        self.report_error(DiagnosticCode::TarGpuIncompatibleSignature,
            format!(
                "Variable '{}' has type '{}' which is not GPU-compatible: only numeric primitives, booleans, and GPU types may be used inside a 'gpu fn'",
                name, inferred_type
            ),
            span,
        );
    }

    /// Rejects a `gpu let` / `gpu var` binding whose type does not implement the
    /// `Accelerable` trait, and therefore cannot be made gpu-resident.
    ///
    /// Gating is by trait dispatch (see [`is_accelerable`]); host bindings are
    /// never constrained. Also validates literal array elements against i32 range.
    fn check_gpu_residency_type(
        &mut self,
        decl: &VariableDeclaration,
        inferred_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if decl.residency != BindingResidency::Gpu {
            return;
        }
        if matches!(inferred_type.kind, TypeKind::Error) {
            return;
        }

        if is_accelerable(
            &inferred_type.kind,
            &self.type_table.global_type_definitions,
        ) {
            self.check_gpu_i32_range_literal(decl, inferred_type, context);
            return;
        }
        self.report_error(
            DiagnosticCode::TarGpuTypeNotAccelerable,
            format!(
                "'{}' does not implement 'Accelerable' and cannot be gpu-resident.",
                inferred_type
            ),
            span,
        );
    }

    /// Rejects an `f16` value on the host path. `f16` is a GPU-only scalar with
    /// no Cranelift representation, so it is admitted only in a `gpu let`/`gpu
    /// var` binding (gpu-resident) or inside a `gpu fn` body (a kernel-body
    /// value). A plain host `let`/`var` carrying it — directly or as a
    /// collection element — is a compile error.
    fn check_host_f16(
        &mut self,
        decl: &VariableDeclaration,
        inferred_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if decl.residency == BindingResidency::Gpu || context.in_gpu_function {
            return;
        }
        if !type_mentions_f16(&inferred_type.kind) {
            return;
        }
        self.report_error(DiagnosticCode::TarGpuParallelConstruct,
            format!(
                "'{}' uses 'f16', a GPU-only type with no host representation; use it inside a 'gpu' binding or a 'gpu fn'/'gpu forall'",
                inferred_type
            ),
            span,
        );
    }

    /// Checks that a gpu-resident variable with a literal integer array initializer
    /// does not contain int (i64) values outside the i32 range.
    /// Non-literal arrays and non-integer element types pass silently.
    ///
    /// This check is a fail-fast path for provably constant array elements.
    /// Runtime expressions that compute out-of-range values are caught by the
    /// narrowing validation in the runtime during buffer upload.
    fn check_gpu_i32_range_literal(
        &mut self,
        decl: &VariableDeclaration,
        inferred_type: &Type,
        context: &Context,
    ) {
        let Some(init) = &decl.initializer else {
            return;
        };

        let inferred_elem_expr = match &inferred_type.kind {
            TypeKind::Array(elem_expr, _) => elem_expr.as_ref(),
            TypeKind::Custom(name, Some(args)) => {
                if BuiltinCollectionKind::from_name(name) != Some(BuiltinCollectionKind::Array) {
                    return;
                }
                if args.is_empty() {
                    return;
                }
                &args[0]
            }
            _ => return,
        };

        self.check_gpu_i32_range_array_expr(init, inferred_elem_expr, context);
    }

    /// Validates that a literal integer array expression does not contain values
    /// outside the 32-bit device lane a 64-bit element is narrowed into (the
    /// range the GPU wire format checks at upload). Used by both variable
    /// initializers and reassignments. Elements that are never narrowed and
    /// non-literal arrays pass silently.
    /// The elem_expr is a type expression (Expression with ExpressionKind::Type or Identifier).
    pub(crate) fn check_gpu_i32_range_array_expr(
        &mut self,
        expr: &Expression,
        elem_expr: &Expression,
        context: &Context,
    ) {
        let ExpressionKind::Array(elements, _) = &expr.node else {
            return;
        };

        let Some(wire) =
            resolve_element_type_kind(elem_expr).and_then(|kind| buffer_element_wire(&kind))
        else {
            return;
        };
        let Some((min, max)) = wire.conversion.checked_range() else {
            return;
        };

        for (elem_idx, elem) in elements.iter().enumerate() {
            let Some(val) = Self::try_eval_const_int_with_context(elem, context) else {
                continue;
            };
            if val < min || val > max {
                self.report_error(
                    DiagnosticCode::TarGpuValueOutOfRange,
                    format!(
                        "Array element {} has value {} which exceeds {} range [{}, {}]; \
                        use Array<{}, N> for explicit 32-bit GPU storage",
                        elem_idx,
                        val,
                        wire.device.name(),
                        min,
                        max,
                        wire.device.name()
                    ),
                    elem.span,
                );
            }
        }
    }

    pub(crate) fn check_shadowing(
        &mut self,
        name: &str,
        is_mutable: bool,
        is_constant: bool,
        context: &Context,
        span: Span,
    ) {
        // Find existing info in any scope
        let existing_info = context.resolve_info(name);

        // Rule 4: Constant shadowing is not allowed in any scope (declaring a NEW constant)
        if is_constant && existing_info.is_some() {
            self.report_error(
                DiagnosticCode::TypShadowingNotAllowed,
                format!(
                    "Cannot shadow existing variable/constant '{}' with a constant.",
                    name
                ),
                span,
            );
            return;
        }

        // Rule 5: Cannot shadow an existing constant (declaring any variable shadowing a constant)
        if let Some(existing) = existing_info {
            if existing.is_constant {
                self.report_error(
                    DiagnosticCode::TypShadowingNotAllowed,
                    format!("Cannot shadow constant '{}'.", name),
                    span,
                );
                return;
            }
        }

        // Check for same-scope shadowing rules
        if let Some(current_scope) = context.scopes.last() {
            if let Some(existing_info) = current_scope.get(name) {
                // Rule 2: var may not shadow in the same scope
                if is_mutable {
                    self.report_error(DiagnosticCode::TypShadowingNotAllowed,
                        format!("Variable '{}' is already defined in this scope. 'var' cannot shadow existing variables.", name),
                        span,
                    );
                }
                // Rule 3: switching let <-> var via shadowing in the same scope is not allowed
                // We already know new is not mutable (from Rule 2 check above), so new is 'let'.
                // If existing is 'var' (mutable), then we are switching var -> let, which is disallowed.
                else if existing_info.mutable {
                    self.report_error(DiagnosticCode::TypShadowingNotAllowed,
                        format!("Cannot shadow mutable variable '{}' with an immutable one in the same scope.", name),
                        span,
                    );
                }
                // Rule 1: let shadowing let is allowed (implicit else)
            }
        }
    }

    /// Determines the type of a variable from its initializer and/or type annotation.
    ///
    /// When both are present, validates compatibility and returns the declared type.
    /// Warns when immutable variables are unnecessarily declared optional.
    pub(crate) fn determine_variable_type(
        &mut self,
        decl: &VariableDeclaration,
        context: &mut Context,
        span: Span,
    ) -> Type {
        // A wide integer annotation (`i128`/`u64`/`u128`) exempts its literal
        // initializer from the default-`int` (i64) range check, which otherwise
        // runs when the initializer is inferred below.
        if let (Some(type_expr), Some(init)) = (&decl.typ, &decl.initializer) {
            let declared = self.resolve_type_expression(type_expr, context);
            if int_type_exceeds_i64(&declared.kind) {
                self.mark_wide_typed_int_literal(init);
            }
        }

        let inferred_type = if let Some(init) = &decl.initializer {
            // A gpu-resident initializer is checked against the device's scalar
            // widths: its values are uploaded to a buffer a kernel reads, so an
            // unconstrained float literal in it takes the device default, not
            // the host one.
            let outer_resident = context.in_gpu_resident_initializer;
            context.in_gpu_resident_initializer =
                outer_resident || decl.residency == BindingResidency::Gpu;
            let inferred = self.infer_expression(init, context);
            context.in_gpu_resident_initializer = outer_resident;
            inferred
        } else if let Some(type_expr) = &decl.typ {
            self.resolve_type_expression(type_expr, context)
        } else {
            self.report_error(
                DiagnosticCode::TypTypeInference,
                format!("Cannot infer type for variable '{}'", decl.name),
                span,
            );
            make_type(TypeKind::Error)
        };

        // If both type annotation and initializer exist, check compatibility
        if let (Some(type_expr), Some(init)) = (&decl.typ, &decl.initializer) {
            let declared_type = self.resolve_type_expression(type_expr, context);
            // The annotation is a declared width, so a literal initializer takes
            // it before the two are compared — `let x f32 = 3.14` narrows rather
            // than reporting an f64 the source never asked for.
            let inferred_type = self
                .narrow_float_literals(init, &declared_type, &inferred_type, context)
                .unwrap_or(inferred_type);
            let inferred_type = self
                .widen_int_literals(init, &declared_type, &inferred_type)
                .unwrap_or(inferred_type);
            if !self.are_compatible(&declared_type, &inferred_type, context)
                || !self.type_arguments_name_the_same_parameters(
                    &declared_type,
                    &inferred_type,
                    context,
                )
            {
                // Check for list literal compatibility (e.g. [1] -> [i16])
                let mut compatible = false;
                if let (TypeKind::List(target_inner), ExpressionKind::List(elements)) =
                    (&declared_type.kind, &init.node)
                {
                    if let Ok(target_inner_type) = self.extract_type_from_expression(target_inner) {
                        if self.is_integer(&target_inner_type) {
                            compatible =
                                self.check_integer_list_literal(elements, &target_inner_type);
                        }
                    }
                }

                if !compatible {
                    self.report_error(
                        DiagnosticCode::TypTypeMismatch,
                        format!(
                            "Type mismatch for variable '{}': expected {}, got {}",
                            decl.name, declared_type, inferred_type
                        ),
                        init.span,
                    );
                }
            } else {
                // Check for warning: assigning non-optional to optional immutable variable
                if let TypeKind::Option(_) = &declared_type.kind {
                    if !matches!(
                        decl.declaration_type,
                        VariableDeclarationType::Mutable | VariableDeclarationType::Unmarked
                    ) {
                        // If inferred type is NOT optional (and not None), warn
                        if !matches!(inferred_type.kind, TypeKind::Option(_)) {
                            self.report_warning(
                                DiagnosticCode::TypUnnecessaryOptionalDeclaration,
                                "Unnecessary Optional Declaration".to_string(),
                                format!(
                                    "Unnecessary optional declaration for variable '{}'",
                                    decl.name
                                ),
                                type_expr.span,
                                Some(format!(
                                    "Variable '{}' is immutable and its initializer is not optional. Remove `?` from the type to simplify.",
                                    decl.name
                                )),
                            );
                        }
                    }
                }
            }
            return declared_type;
        }

        inferred_type
    }

    /// Records an integer-literal initializer (bare or directly signed) as
    /// wide-typed so it is exempt from the default-`int` (i64) range check.
    fn mark_wide_typed_int_literal(&mut self, init: &Expression) {
        match &init.node {
            ExpressionKind::Literal(Literal::Integer(_)) => {
                self.wide_typed_int_literals.insert(init.id);
            }
            ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand) => {
                if let ExpressionKind::Literal(Literal::Integer(_)) = &operand.node {
                    // Only the range check is relaxed here; the operand takes
                    // the declared width from the widening pass, which is what
                    // lets the sign be applied at that width.
                    self.wide_typed_int_literals.insert(operand.id);
                }
            }
            _ => {}
        }
    }
}

/// Returns `true` if `kind` is an integer type whose range exceeds `i64`
/// (`i128`/`u64`/`u128`), for which a literal may legitimately exceed `i64::MAX`.
fn int_type_exceeds_i64(kind: &TypeKind) -> bool {
    matches!(kind, TypeKind::I128 | TypeKind::U64 | TypeKind::U128)
}

/// A folded integer `value` as a literal of the binding type `kind`: an integer
/// of that width, or a float when an integer was written for a float binding.
fn integer_constant(kind: &TypeKind, value: i128) -> Option<Literal> {
    if let Some(integer) = crate::ast::literal::IntegerLiteral::from_type_kind(kind, value) {
        return Some(Literal::Integer(integer));
    }
    match kind {
        TypeKind::Float | TypeKind::F64 => Some(Literal::Float(
            crate::ast::literal::FloatLiteral::F64((value as f64).to_bits()),
        )),
        TypeKind::F32 => Some(Literal::Float(crate::ast::literal::FloatLiteral::F32(
            (value as f32).to_bits(),
        ))),
        _ => None,
    }
}

/// `float` with its sign flipped, at the same width.
fn negated_float(float: crate::ast::literal::FloatLiteral) -> crate::ast::literal::FloatLiteral {
    use crate::ast::literal::FloatLiteral;
    match float {
        FloatLiteral::F32(bits) => FloatLiteral::F32((-f32::from_bits(bits)).to_bits()),
        FloatLiteral::F64(bits) => FloatLiteral::F64((-f64::from_bits(bits)).to_bits()),
    }
}
