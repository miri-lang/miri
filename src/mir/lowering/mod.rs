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

pub mod context;
pub mod control_flow;
pub mod expression;
pub mod helpers;
pub mod statement;
pub mod variable;

use crate::ast::expression::ExpressionKind;
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

    // Resolve return type: explicit annotation > type checker inference > void
    let ret_ty = if let Some(ret_expr) = ret_type_expr {
        resolve_type(tc, ret_expr)
    } else if let Some(ty) = tc.get_variable_type(name) {
        if let TypeKind::Function(func) = &ty.kind {
            if let Some(rt) = &func.return_type {
                resolve_type(tc, rt)
            } else {
                Type::new(TypeKind::Void, ast_func.span)
            }
        } else {
            Type::new(TypeKind::Void, ast_func.span)
        }
    } else {
        Type::new(TypeKind::Void, ast_func.span)
    };

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

    // Lower parameters
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

    // Resolve return type with generic substitution applied
    let ret_ty = if let Some(ret_expr) = ret_type_expr {
        apply_generic_sub(&resolve_type(tc, ret_expr), subs)
    } else if let Some(ty) = tc.get_variable_type(name) {
        if let TypeKind::Function(func) = &ty.kind {
            if let Some(rt) = &func.return_type {
                apply_generic_sub(&resolve_type(tc, rt), subs)
            } else {
                Type::new(TypeKind::Void, ast_func.span)
            }
        } else {
            Type::new(TypeKind::Void, ast_func.span)
        }
    } else {
        Type::new(TypeKind::Void, ast_func.span)
    };

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

    // Lower parameters with generic substitution
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

    let ret_ty = if let Some(ret_expr) = ret_type_expr {
        resolve_type(tc, ret_expr)
    } else {
        Type::new(TypeKind::Void, ast_method.span)
    };

    let execution_model = resolve_execution_model(props);

    // Initial arg_count = 1 (self) + explicit params.
    // The allocator is added to arg_count below but NOT to variable_map.
    let body = Body::new(params.len() + 1, ast_method.span, execution_model);
    let mut ctx = LoweringContext::new(body, tc, is_release);

    // Populate generic type parameter names (captures both method-own generics
    // and class-level generics that appear in param/return types, e.g. T in List<T>).
    ctx.body.type_params = collect_type_params(decl, tc);

    // _0: Return value
    ctx.body
        .new_local(LocalDecl::new(ret_ty.clone(), ast_method.span));

    // _1: self parameter (the class instance, registered in variable_map)
    ctx.push_param("self".to_string(), self_type, ast_method.span);

    // Remaining explicit parameters (registered in variable_map)
    for param in params.iter() {
        let param_ty = resolve_type(tc, &param.typ);
        ctx.push_param(param.name.clone(), param_ty, param.typ.span);
    }

    // Inject allocator into the ABI for call-site compatibility.
    // We MUST register it in variable_map so that method-to-method calls can pass the allocator.
    let allocator_decl = LocalDecl::new(Type::new(TypeKind::Int, ast_method.span), ast_method.span);
    let alloc_local = ctx.body.new_local(allocator_decl);
    ctx.variable_map.insert("allocator".into(), alloc_local);
    ctx.body.arg_count += 1;

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
        TypeKind::List(inner) | TypeKind::Set(inner) => {
            if let EK::Type(inner_ty, _) = &inner.node {
                collect_generic_names_from_type(inner_ty, params);
            }
        }
        TypeKind::Array(inner, _) => {
            if let EK::Type(inner_ty, _) = &inner.node {
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
        let Some(guard) = &param.guard else {
            continue;
        };
        let Some(&param_local) = ctx.variable_map.get(param.name.as_str()) else {
            continue;
        };
        let ExpressionKind::Guard(guard_op, guard_value) = &guard.node else {
            continue;
        };

        let guard_val = lower_expression(ctx, guard_value, None)?;

        let bin_op = match guard_op {
            crate::ast::operator::GuardOp::GreaterThan => BinOp::Gt,
            crate::ast::operator::GuardOp::GreaterThanEqual => BinOp::Ge,
            crate::ast::operator::GuardOp::LessThan => BinOp::Lt,
            crate::ast::operator::GuardOp::LessThanEqual => BinOp::Le,
            crate::ast::operator::GuardOp::NotEqual => BinOp::Ne,
            _ => continue,
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

        let continue_bb = ctx.new_basic_block();
        let fail_bb = ctx.new_basic_block();

        ctx.set_terminator(Terminator::new(
            TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::new(check_result)),
                targets: vec![(Discriminant::bool_true(), continue_bb)],
                otherwise: fail_bb,
            },
            guard.span,
        ));

        ctx.set_current_block(fail_bb);
        ctx.set_terminator(Terminator::new(TerminatorKind::Unreachable, guard.span));

        ctx.set_current_block(continue_bb);
    }
    Ok(())
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
