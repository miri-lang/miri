// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR lowering - converts AST to MIR (Mid-level Intermediate Representation).
//!
//! This module is organized into focused sub-modules:
//! - `context`: Lowering context and state management
//! - `control_flow`: Control flow constructs (if, while, for, break, continue)
//! - `expression`: Expression lowering (~1600 lines)
//! - `statement`: Statement lowering (~350 lines)
//! - `variable`: Variable declaration lowering
//! - `helpers`: Utility functions (resolve_type, bind_pattern, etc.)

pub mod constructors;
pub mod context;
pub mod control_flow;
pub mod dispatch;
pub mod expression;
pub mod gpu_for;
pub mod gpu_frame;
pub mod helpers;
pub mod loops;
pub mod statement;
pub mod variable;

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::lambda::LambdaInfo;
use crate::mir::{
    BinOp, Body, Constant, Discriminant, ExecutionModel, LocalDecl, Operand, Place, Rvalue,
    StatementKind as MirStatementKind, Terminator, TerminatorKind,
};
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

// Re-export commonly used items from submodules
pub use context::LoweringContext;
pub use expression::lower_expression;
pub use helpers::{bind_pattern, literal_to_u128, lower_as_return, lower_to_local, resolve_type};
pub use statement::lower_statement;

/// Lower an AST function declaration to a MIR Body.
///
/// This is the main entry point for MIR lowering. It creates a lowering context,
/// processes parameters, emits guard checks, and lowers the function body.
///
/// # Arguments
///
/// * `ast_func` - The AST statement containing the function declaration
/// * `tc` - The type checker, used to resolve types and look up definitions
/// * `is_release` - Whether this is a release build (strips debug names)
/// * `inject_allocator` - Whether to inject an implicit allocator parameter
///
/// # Errors
///
/// Returns `LoweringError` if the statement is not a function declaration,
/// if expression lowering fails, or if the resulting MIR fails validation.
/// Resolve a function's return type: explicit annotation, then the type
/// checker's inferred function type, else `void`.
fn resolve_function_return_type(
    tc: &TypeChecker,
    ret_type_expr: Option<&Expression>,
    name: &str,
    span: crate::error::syntax::Span,
) -> Type {
    if let Some(ret_expr) = ret_type_expr {
        return resolve_type(tc, ret_expr);
    }
    match tc.get_variable_type(name).map(|t| &t.kind) {
        Some(TypeKind::Function(func)) => match &func.return_type {
            Some(rt) => resolve_type(tc, rt),
            None => Type::new(TypeKind::Void, span),
        },
        _ => Type::new(TypeKind::Void, span),
    }
}

pub fn lower_function(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    let StatementKind::FunctionDeclaration(decl) = &ast_func.node else {
        return Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration".to_string(),
            ast_func.span,
        ));
    };
    let name = &decl.name;
    let params = &decl.params;
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let ret_ty = resolve_function_return_type(tc, ret_type_expr.as_deref(), name, ast_func.span);

    let execution_model = resolve_execution_model(props);

    // Initialize lowering context
    let body = Body::new(params.len(), ast_func.span, execution_model);
    let mut ctx = LoweringContext::new(body, tc, is_release);

    // Populate generic type parameter names so that `is_managed_type` can
    // distinguish unresolved generic placeholders from concrete user types.
    ctx.body.type_params = collect_type_params(decl, tc);

    // _0: Return value
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_func.span));

    // Lower parameters and record out-param flags.
    ctx.body.out_params = params.iter().map(|p| p.is_out).collect();
    for param in params.iter() {
        let param_ty = resolve_type(tc, &param.typ);
        ctx.push_param(param.name.clone(), param_ty, param.typ.span);
    }

    // Implicit Allocator Injection — supports the "Call Site Allocator Injection" strategy.
    if inject_allocator {
        inject_allocator_param(&mut ctx, name, ast_func.span);
    }

    // Emit guard checks for parameters with guards
    emit_parameter_guards(&mut ctx, params)?;

    // Lower body with support for implicit return
    if let Some(body_box) = body_stmt {
        lower_as_return(&mut ctx, body_box, &ret_ty)?;
    }

    finalize_body(&mut ctx, ast_func.span)
}

/// Apply a generic substitution mapping to a `Type`, replacing generic parameters
/// with their concrete counterparts.
///
/// Handles two representations that appear in `resolve_type` output:
/// - `TypeKind::Generic("T", ...)` - explicit generic placeholder
/// - `TypeKind::Custom("T", None)` - generic param written as a plain identifier
pub(crate) fn apply_generic_sub(ty: &Type, subs: &HashMap<String, Type>) -> Type {
    match &ty.kind {
        TypeKind::Generic(name, _, _) => subs.get(name).cloned().unwrap_or_else(|| ty.clone()),
        TypeKind::Custom(name, None) if subs.contains_key(name.as_str()) => {
            subs[name.as_str()].clone()
        }
        _ => ty.clone(),
    }
}

/// Lower a generic function with concrete type substitutions to produce a
/// specialised MIR Body.
///
/// This is used by the monomorphisation pass in the pipeline after all call
/// sites have been lowered. `mangled_name` is the already-computed symbol
/// (e.g. `identity__int`) and `subs` maps each generic parameter name to its
/// concrete type.
/// Resolve a generic function's return type (same precedence as
/// [`resolve_function_return_type`]) with the generic substitution applied.
fn resolve_generic_return_type(
    tc: &TypeChecker,
    ret_type_expr: Option<&Expression>,
    name: &str,
    span: crate::error::syntax::Span,
    subs: &HashMap<String, Type>,
) -> Type {
    if let Some(ret_expr) = ret_type_expr {
        return apply_generic_sub(&resolve_type(tc, ret_expr), subs);
    }
    match tc.get_variable_type(name).map(|t| &t.kind) {
        Some(TypeKind::Function(func)) => match &func.return_type {
            Some(rt) => apply_generic_sub(&resolve_type(tc, rt), subs),
            None => Type::new(TypeKind::Void, span),
        },
        _ => Type::new(TypeKind::Void, span),
    }
}

pub fn lower_generic_instantiation(
    ast_func: &Statement,
    tc: &TypeChecker,
    is_release: bool,
    inject_allocator: bool,
    subs: &HashMap<String, Type>,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    let StatementKind::FunctionDeclaration(decl) = &ast_func.node else {
        return Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration".to_string(),
            ast_func.span,
        ));
    };
    let name = &decl.name;
    let params = &decl.params;
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let ret_ty =
        resolve_generic_return_type(tc, ret_type_expr.as_deref(), name, ast_func.span, subs);

    let execution_model = resolve_execution_model(props);

    let body = Body::new(params.len(), ast_func.span, execution_model);
    let mut ctx = LoweringContext::new(body, tc, is_release);

    // For an instantiated generic, `subs` maps each generic name to its concrete
    // type — those names no longer remain as unresolved placeholders after
    // substitution. Populate type_params with the original names anyway so that
    // any types not yet substituted (e.g. nested generics) are handled correctly.
    ctx.body.type_params = subs.keys().cloned().collect();

    // _0: Return value (concrete type)
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_func.span));

    // Lower parameters with generic substitution and record out-param flags.
    ctx.body.out_params = params.iter().map(|p| p.is_out).collect();
    for param in params.iter() {
        let param_ty = apply_generic_sub(&resolve_type(tc, &param.typ), subs);
        ctx.push_param(param.name.clone(), param_ty, param.typ.span);
    }

    if inject_allocator {
        inject_allocator_param(&mut ctx, name, ast_func.span);
    }

    emit_parameter_guards(&mut ctx, params)?;

    if let Some(body_box) = body_stmt {
        lower_as_return(&mut ctx, body_box, &ret_ty)?;
    }

    finalize_body(&mut ctx, ast_func.span)
}

/// Lower a stdlib class method to a MIR Body.
///
/// Unlike [`lower_function`], this variant:
/// - Prepends an implicit `self` parameter (registered in `variable_map`)
/// - Registers the allocator in the function ABI (`body.arg_count`) but NOT in
///   the lowering context's `variable_map`
///
/// Keeping the allocator out of `variable_map` prevents the auto-injector from
/// appending it to calls to runtime C functions inside the method body. Those C
/// functions do not accept an allocator parameter.
///
/// # Arguments
///
/// * `ast_method` - The AST statement containing the method declaration
/// * `self_type` - The type of the implicit `self` parameter
/// * `tc` - The type checker, used to resolve types and look up definitions
/// * `is_release` - Whether this is a release build (strips debug names)
///
/// # Errors
///
/// Returns `LoweringError` if the statement is not a function declaration,
/// if expression lowering fails, or if the resulting MIR fails validation.
/// Inject enum/class-level generic names (e.g. `T`, `E` from `Result<T, E>`)
/// into `type_params`. `collect_type_params` only catches `TypeKind::Generic`,
/// missing names that resolve to `Custom("T", None)` for enum/class methods;
/// reading them from the type definition closes that gap so Perceus does not
/// treat unresolved placeholders as concrete heap-managed types.
fn inject_class_level_generics(ctx: &mut LoweringContext, self_type: &Type, tc: &TypeChecker) {
    let TypeKind::Custom(class_name, _) = &self_type.kind else {
        return;
    };
    let Some(type_def) = tc.type_definitions().get(class_name.as_str()) else {
        return;
    };
    let generics = match type_def {
        crate::type_checker::context::TypeDefinition::Enum(ed) => ed.generics.as_deref(),
        crate::type_checker::context::TypeDefinition::Class(cd) => cd.generics.as_deref(),
        _ => None,
    };
    if let Some(gens) = generics {
        for gen in gens {
            ctx.body.type_params.insert(gen.name.clone());
        }
    }
}

pub fn lower_class_method(
    ast_method: &Statement,
    self_type: Type,
    tc: &TypeChecker,
    is_release: bool,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    let StatementKind::FunctionDeclaration(decl) = &ast_method.node else {
        return Err(LoweringError::unsupported_statement(
            "Expected FunctionDeclaration for class method".to_string(),
            ast_method.span,
        ));
    };
    let params = &decl.params;
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let ret_ty = ret_type_expr.as_deref().map_or_else(
        || Type::new(TypeKind::Void, ast_method.span),
        |e| resolve_type(tc, e),
    );

    let execution_model = resolve_execution_model(props);

    // arg_count = 1 (self) + explicit params; the allocator is counted below but
    // not added to variable_map.
    let body = Body::new(params.len() + 1, ast_method.span, execution_model);
    let mut ctx = LoweringContext::new(body, tc, is_release);

    // Populate generic type parameter names (captures both method-own generics
    // and class-level generics that appear in param/return types, e.g. T in List<T>).
    ctx.body.type_params = collect_type_params(decl, tc);

    inject_class_level_generics(&mut ctx, &self_type, tc);

    // _0: Return value
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_method.span));

    // _1: self parameter (the class instance, registered in variable_map)
    ctx.push_param("self".to_string(), self_type, ast_method.span);

    // Remaining explicit parameters (registered in variable_map). ABI param 0
    // is `self` (never `out`); explicit params follow at 1..=N. The allocator
    // is appended below as a non-out ABI param.
    let mut out_params = Vec::with_capacity(params.len() + 2);
    out_params.push(false);
    for param in params.iter() {
        let param_ty = resolve_type(tc, &param.typ);
        ctx.push_param(param.name.clone(), param_ty, param.typ.span);
        out_params.push(param.is_out);
    }

    // Inject allocator into the ABI for call-site compatibility.
    // We MUST register it in variable_map so that method-to-method calls can pass the allocator.
    let allocator_decl = LocalDecl::new(Type::new(TypeKind::Int, ast_method.span), ast_method.span);
    let alloc_local = ctx.body.new_local(allocator_decl);
    ctx.variable_map.insert("allocator".into(), alloc_local);
    ctx.body.arg_count += 1;
    out_params.push(false);
    ctx.body.out_params = out_params;

    // Lower body
    if let Some(body_box) = body_stmt {
        lower_as_return(&mut ctx, body_box, &ret_ty)?;
    }

    finalize_body(&mut ctx, ast_method.span)
}

/// Resolve execution model from function properties.
fn resolve_execution_model(props: &crate::ast::common::FunctionProperties) -> ExecutionModel {
    if props.is_gpu {
        ExecutionModel::GpuKernel
    } else if props.is_async {
        ExecutionModel::Async
    } else {
        ExecutionModel::Cpu
    }
}

/// Collect generic type parameter names from a function declaration.
///
/// Extracts names from:
/// - Explicit generic declarations in `decl.generics` (e.g. `fn foo<T, K>(...)`)
/// - `TypeKind::Generic` names found in parameter types (captures class-level
///   generics that appear in method signatures, e.g. `T` in `List<T>::push(item: T)`)
/// - `TypeKind::Generic` names found in the return type
///
/// The resulting set is stored in `Body::type_params` and used by `is_managed_type`
/// to distinguish unresolved generic placeholders from concrete user-defined types.
fn collect_type_params(
    decl: &crate::ast::statement::FunctionDeclarationData,
    tc: &TypeChecker,
) -> std::collections::HashSet<String> {
    let mut params = std::collections::HashSet::new();

    // Explicit generic declarations (e.g., `fn foo<T>(...)`)
    if let Some(gens) = &decl.generics {
        for gen_expr in gens {
            if let ExpressionKind::GenericType(name_expr, _, _) = &gen_expr.node {
                if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                    params.insert(name.clone());
                }
            }
        }
    }

    // Generic names appearing in parameter types (catches class-level generics)
    for param in &decl.params {
        collect_generic_names_from_type(&resolve_type(tc, &param.typ), &mut params);
    }

    // Generic names appearing in the return type
    if let Some(ret_expr) = &decl.return_type {
        collect_generic_names_from_type(&resolve_type(tc, ret_expr), &mut params);
    }

    params
}

/// Recursively collect `TypeKind::Generic` parameter names from a resolved type.
fn collect_generic_names_from_type(ty: &Type, params: &mut std::collections::HashSet<String>) {
    use crate::ast::expression::ExpressionKind as EK;
    match &ty.kind {
        TypeKind::Generic(name, _, _) => {
            params.insert(name.clone());
        }
        TypeKind::Option(inner) => collect_generic_names_from_type(inner, params),
        TypeKind::Linear(inner) => collect_generic_names_from_type(inner, params),
        // Canonical collection variants may appear when resolve_type reads a raw
        // type expression from the parser before normalization. Recurse into their
        // element/key/value type expressions to collect any generic names.
        TypeKind::List(elem) | TypeKind::Set(elem) => {
            if let EK::Type(inner_ty, _) = &elem.node {
                collect_generic_names_from_type(inner_ty, params);
            }
        }
        TypeKind::Array(elem, _) => {
            if let EK::Type(inner_ty, _) = &elem.node {
                collect_generic_names_from_type(inner_ty, params);
            }
        }
        TypeKind::Map(k, v) => {
            if let EK::Type(k_ty, _) = &k.node {
                collect_generic_names_from_type(k_ty, params);
            }
            if let EK::Type(v_ty, _) = &v.node {
                collect_generic_names_from_type(v_ty, params);
            }
        }
        TypeKind::Custom(_, Some(args)) => {
            for arg in args {
                if let EK::Type(inner_ty, _) = &arg.node {
                    collect_generic_names_from_type(inner_ty, params);
                }
            }
        }
        TypeKind::Custom(_, None) => {}
        _ => {}
    }
}

/// Inject an allocator parameter into the lowering context.
///
/// For `main`, creates a local variable initialized to 0 (cannot inject a parameter
/// as it would break the entry point signature). For all other functions, appends
/// an additional parameter to the function signature.
fn inject_allocator_param(
    ctx: &mut LoweringContext,
    function_name: &str,
    span: crate::error::syntax::Span,
) {
    let allocator_type = Type::new(TypeKind::Int, span);

    if function_name == "main" {
        // For main, create a local variable instead of a parameter to preserve
        // the entry point ABI. Initialize to 0 to avoid uninitialized reads.
        let alloc_local = ctx.push_local("allocator".to_string(), allocator_type.clone(), span);

        let dummy_allocator = Operand::Constant(Box::new(Constant {
            span,
            ty: allocator_type,
            literal: crate::ast::literal::Literal::Integer(
                crate::ast::literal::IntegerLiteral::I32(0),
            ),
        }));

        ctx.push_statement(crate::mir::Statement {
            kind: MirStatementKind::Assign(Place::new(alloc_local), Rvalue::Use(dummy_allocator)),
            span,
        });
    } else {
        ctx.push_param("allocator".to_string(), allocator_type, span);
        ctx.body.arg_count += 1;
    }
}

/// Emit guard checks for parameters that have guard conditions.
///
/// For each parameter with a guard (e.g., `n > 0`), emits a comparison followed
/// by a conditional branch to an unreachable block on failure.
fn emit_parameter_guards(
    ctx: &mut LoweringContext,
    params: &[crate::ast::common::Parameter],
) -> Result<(), LoweringError> {
    for param in params {
        emit_param_guard(ctx, param)?;
    }
    Ok(())
}

/// Emit the comparison + fail-on-false branch for a single guarded parameter.
/// Parameters without a (supported) guard are left untouched.
fn emit_param_guard(
    ctx: &mut LoweringContext,
    param: &crate::ast::common::Parameter,
) -> Result<(), LoweringError> {
    let Some(guard) = &param.guard else {
        return Ok(());
    };
    let Some(&param_local) = ctx.variable_map.get(param.name.as_str()) else {
        return Ok(());
    };
    let ExpressionKind::Guard(guard_op, guard_value) = &guard.node else {
        return Ok(());
    };

    let guard_val = lower_expression(ctx, guard_value, None)?;
    let Some(bin_op) = guard_op_to_binop(guard_op) else {
        return Ok(());
    };

    let check_result = ctx.push_temp(Type::new(TypeKind::Boolean, guard.span), guard.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            Place::new(check_result),
            Rvalue::BinaryOp(
                bin_op,
                Box::new(Operand::Copy(Place::new(param_local))),
                Box::new(guard_val),
            ),
        ),
        span: guard.span,
    });
    emit_guard_fail_branch(ctx, check_result, guard.span);
    Ok(())
}

/// Map a guard operator to its MIR comparison op (None if unsupported).
fn guard_op_to_binop(op: &crate::ast::operator::GuardOp) -> Option<BinOp> {
    match op {
        crate::ast::operator::GuardOp::GreaterThan => Some(BinOp::Gt),
        crate::ast::operator::GuardOp::GreaterThanEqual => Some(BinOp::Ge),
        crate::ast::operator::GuardOp::LessThan => Some(BinOp::Lt),
        crate::ast::operator::GuardOp::LessThanEqual => Some(BinOp::Le),
        crate::ast::operator::GuardOp::NotEqual => Some(BinOp::Ne),
        _ => None,
    }
}

/// Branch on `check_result`: continue when true, else jump to an unreachable
/// fail block. Leaves the current block at the continue path.
fn emit_guard_fail_branch(
    ctx: &mut LoweringContext,
    check_result: crate::mir::Local,
    span: crate::error::syntax::Span,
) {
    let continue_bb = ctx.new_basic_block();
    let fail_bb = ctx.new_basic_block();
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(check_result)),
            targets: vec![(Discriminant::bool_true(), continue_bb)],
            otherwise: fail_bb,
        },
        span,
    ));
    ctx.set_current_block(fail_bb);
    ctx.set_terminator(Terminator::new(TerminatorKind::Unreachable, span));
    ctx.set_current_block(continue_bb);
}

/// Finalize the lowering context: pop root scope, ensure termination, and validate.
///
/// This shared logic is used by both [`lower_function`] and [`lower_class_method`]
/// to avoid duplicating the post-lowering finalization sequence.
///
/// # Errors
///
/// Returns `LoweringError` if the MIR body fails validation.
fn finalize_body(
    ctx: &mut LoweringContext,
    span: crate::error::syntax::Span,
) -> Result<(Body, Vec<LambdaInfo>), LoweringError> {
    // Pop root scope variables if falling through
    if ctx.body.basic_blocks[ctx.current_block.0]
        .terminator
        .is_none()
    {
        ctx.pop_scope(span);
    }

    // Ensure the last block has a terminator
    let last_block_idx = ctx.current_block.0;
    if ctx.body.basic_blocks[last_block_idx].terminator.is_none() {
        ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    }

    // Validate the body
    if let Err(msg) = ctx.body.validate() {
        return Err(LoweringError::custom(
            format!("MIR Validation Error: {}", msg),
            span,
            None,
        ));
    }

    let body = std::mem::replace(&mut ctx.body, Body::new(0, span, ExecutionModel::Cpu));
    let lambda_bodies = std::mem::take(&mut ctx.lambda_bodies);
    Ok((body, lambda_bodies))
}
