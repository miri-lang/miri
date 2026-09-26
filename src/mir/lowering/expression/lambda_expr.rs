// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lambda/closure lowering to MIR.
//!
//! All lambdas — capturing or not — are lowered to **closure structs**, and so
//! is a function declared inside another function's body.
//! A closure struct is a heap allocation: [malloc_ptr][RC][fn_ptr][cap0][cap1]...
//! The lambda variable holds `payload_ptr` (past the 2-word header).
//!
//! Lambda body calling convention:
//!   Local 0      = return value
//!   Local 1      = env_ptr (TypeKind::RawPtr) — implicit first parameter
//!   Local 2..N+1 = user parameters
//!   Local N+2..  = captured values (loaded from env_ptr in codegen)
//!
//! Capture detection: outer-scope variables are added to the lambda context so the
//! body can reference them. After lowering, only those the body MIR references —
//! reads, stores through, indexes with, or passes to a call or kernel launch —
//! are kept as real captures. Unused ones are pruned.

use crate::ast::common::{FunctionProperties, Parameter};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::Statement;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lambda::{CapturedVar, LambdaInfo};
use crate::mir::place::PlaceContext;
use crate::mir::rvalue::AggregateKind;
use crate::mir::symbol::{ClosureKind, Symbol};
use crate::mir::visitor::Visitor;
use crate::mir::{
    BasicBlock, Body, Local, LocalDecl, Operand, Place, Rvalue, StatementKind as MirStatementKind,
    Terminator, TerminatorKind,
};

use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::identifier_expr::lower_self_reference_value;
use crate::mir::lowering::helpers::{lower_as_return, resolve_type};
use crate::mir::lowering::{apply_generic_sub, resolve_execution_model};
use std::collections::HashSet;
use std::rc::Rc;

/// The locals a closure body references: every local it reads, stores
/// through, indexes with, or hands to a call or a kernel launch.
///
/// Writing a local whole is not a reference: the body's copy of an enclosing
/// variable it only ever overwrites needs no value from the environment.
#[derive(Default)]
struct ReferencedLocals {
    locals: HashSet<Local>,
}

impl Visitor for ReferencedLocals {
    fn visit_place(&mut self, place: &Place, context: PlaceContext, location: BasicBlock) {
        if is_reference_to_base(place, context) {
            self.visit_local(place.local, context, location);
        }
        self.visit_projection(place, context, location);
    }

    fn visit_local(&mut self, local: Local, _context: PlaceContext, _location: BasicBlock) {
        self.locals.insert(local);
    }

    fn visit_statement(&mut self, block: BasicBlock, statement: &crate::mir::Statement) {
        match &statement.kind {
            MirStatementKind::DecRef(place) | MirStatementKind::Dealloc(place) => {
                self.visit_local(place.local, PlaceContext::MutatingUse, block);
                self.visit_projection(place, PlaceContext::MutatingUse, block);
            }
            MirStatementKind::Assign(place, rvalue) | MirStatementKind::Reassign(place, rvalue) => {
                self.visit_assign(block, place, rvalue);
            }
            MirStatementKind::IncRef(place) => {
                self.visit_place(place, PlaceContext::NonMutatingUse, block);
            }
            MirStatementKind::StorageLive(_)
            | MirStatementKind::StorageDead(_)
            | MirStatementKind::Nop => {}
        }
    }
}

/// Whether `context` on `place` needs the value its base local already holds.
/// A projected place always does — a store into `b.v` or `xs[i]` goes through
/// the object `b` or `xs` holds — and so does any use that is not a write.
fn is_reference_to_base(place: &Place, context: PlaceContext) -> bool {
    match context {
        PlaceContext::NonMutatingUse => true,
        PlaceContext::MutatingUse => !place.projection.is_empty(),
        PlaceContext::StorageLive | PlaceContext::StorageDead => false,
    }
}

/// Collect every local `body` references (see [`ReferencedLocals`]).
fn collect_referenced_locals(body: &Body) -> HashSet<Local> {
    let mut referenced = ReferencedLocals::default();
    referenced.visit_body(body);
    referenced.locals
}

pub(crate) fn lower_lambda_expr(
    ctx: &mut LoweringContext,
    expr: &Expression,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let ExpressionKind::Lambda(lambda) = &expr.node else {
        unreachable!()
    };
    let closure = ClosureSource {
        symbol: ctx.closure_symbol(ClosureKind::Lambda, expr.id),
        self_name: None,
        params: &lambda.params,
        return_type: lambda.return_type.as_deref(),
        body: &lambda.body,
        properties: &lambda.properties,
        ty: apply_generic_sub(&resolve_type(ctx.type_checker, expr), &ctx.generic_subs),
        span: expr.span,
    };
    lower_closure(ctx, &closure, dest)
}

/// Everything a closure is built from: a lambda expression, or a function
/// declared inside another function's body.
pub(crate) struct ClosureSource<'a> {
    /// The symbol the closure's body is emitted under; unique per compilation.
    pub symbol: Symbol,
    /// The name the body calls itself by, for a named nested function.
    pub self_name: Option<&'a str>,
    pub params: &'a [Parameter],
    pub return_type: Option<&'a Expression>,
    pub body: &'a Statement,
    pub properties: &'a FunctionProperties,
    /// The function type of the value the closure is stored in.
    pub ty: Type,
    pub span: Span,
}

/// Lower `closure`'s body as a separate function and store a closure over it in
/// `dest` (a fresh temporary when `None`).
///
/// The body sees every enclosing local; only those it references become
/// captures. That includes the enclosing `allocator`, so a call the body makes
/// to a Miri function forwards the enclosing function's allocator.
pub(crate) fn lower_closure(
    ctx: &mut LoweringContext,
    closure: &ClosureSource,
    dest: Option<Place>,
) -> Result<Operand, LoweringError> {
    let (body, captures) = lower_closure_body(ctx, closure)?;
    ctx.lambda_bodies.push(LambdaInfo {
        symbol: closure.symbol.clone(),
        body,
        captures: captures.clone(),
    });
    Ok(emit_closure_aggregate(ctx, closure, &captures, dest))
}

/// Lower the closure's body, returning it with the captures it keeps.
fn lower_closure_body(
    ctx: &mut LoweringContext,
    closure: &ClosureSource,
) -> Result<(Body, Vec<CapturedVar>), LoweringError> {
    let span = closure.span;
    // TODO: an unannotated lambda is lowered as `void` here, but the type
    // checker infers its return type from the body's last expression, so a
    // body ending in `x + 1` is called as returning `int` and codegen refuses
    // the two signatures. Read the checker's recorded type for the lambda.
    let ret_ty = match closure.return_type {
        Some(ret_expr) => ctx.declared_type(ret_expr),
        None => Type::new(TypeKind::Void, span),
    };
    let mut lambda_ctx = closure_context(ctx, closure, &ret_ty);

    let tentative_captures = declare_tentative_captures(ctx, &mut lambda_ctx, closure);

    lower_as_return(&mut lambda_ctx, closure.body, &ret_ty)?;

    // Falling off the end must end the root scope's storage exactly as an
    // explicit `return` does: a capture the body assigns owns the value it last
    // stored, and only its `StorageDead` releases it.
    let last_block_idx = lambda_ctx.current_block.0;
    if lambda_ctx.body.basic_blocks[last_block_idx]
        .terminator
        .is_none()
    {
        lambda_ctx.pop_scope(span);
        lambda_ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    }

    let captures = keep_referenced_captures(&mut lambda_ctx.body, &tentative_captures);

    // A body lowered inside this one — a nested lambda, or the thunk a function
    // reference needs — was registered against the inner context and would be
    // discarded with it, leaving codegen a call to a symbol nothing defines.
    ctx.lambda_bodies.append(&mut lambda_ctx.lambda_bodies);

    Ok((lambda_ctx.body, captures))
}

/// A fresh context for the closure's body, holding its return slot, its
/// environment pointer and its parameters, with nothing lowered yet.
fn closure_context<'a>(
    ctx: &LoweringContext<'a>,
    closure: &ClosureSource,
    ret_ty: &Type,
) -> LoweringContext<'a> {
    let span = closure.span;
    let execution_model = resolve_execution_model(closure.properties);

    // arg_count = 1 (env_ptr) + user params. Captures are loaded from the
    // environment, not passed, so they do not count.
    let mut lambda_body = Body::new(1 + closure.params.len(), span, execution_model);

    // Local 0: return value — allocated directly in body before context creation.
    lambda_body.new_local(LocalDecl::new(ret_ty.clone(), span));

    // NOTE: all param locals (1, 2, ...) are allocated via push_param, NOT new_local,
    // so the LoweringContext sees them as proper parameters.
    let mut lambda_ctx = LoweringContext::new(lambda_body, ctx.type_checker, ctx.is_release);
    // Inherit the compilation-wide kernel-name allocator so a kernel lowered
    // inside the lambda body stays deterministic and collision-free.
    lambda_ctx.use_compilation_ids(ctx.compilation_ids.clone());
    // A closure inside an instantiated generic body is part of that
    // instantiation: its types, and the symbols of closures nested in it, are
    // read at the enclosing body's type arguments.
    lambda_ctx.generic_subs = ctx.generic_subs.clone();

    // Local 1: env_ptr (implicit first parameter — pointer to the closure struct payload).
    // We use push_param so it does NOT emit StorageLive.
    let env_ptr = lambda_ctx.push_param(
        "__env_ptr".to_string(),
        Type::new(TypeKind::RawPtr, span),
        span,
    );

    // The environment pointer is the closure itself, so a call through it is a
    // call to this body. A parameter of the same name, bound next, shadows it.
    if let Some(self_name) = closure.self_name {
        lambda_ctx.bind_local_name(self_name.to_string(), env_ptr);
        lambda_ctx
            .self_references
            .insert(env_ptr, closure.ty.clone());
    }

    // Locals 2..N+1: user parameters.
    for param in closure.params {
        let param_ty = ctx.declared_type(&param.typ);
        lambda_ctx.push_param(param.name.clone(), param_ty, param.typ.span);
    }
    lambda_ctx
}

/// Make every enclosing local that is not shadowed by a parameter or by the
/// closure's own name resolvable in the closure body, returning
/// `(name, outer_local, lambda_local)` for each.
///
/// All of them are potential captures; [`keep_referenced_captures`] prunes the ones
/// the body never references.
fn declare_tentative_captures(
    ctx: &LoweringContext,
    lambda_ctx: &mut LoweringContext,
    closure: &ClosureSource,
) -> Vec<(Rc<str>, Local, Local)> {
    let param_names: HashSet<&str> = closure.params.iter().map(|p| p.name.as_str()).collect();

    // Stable ordering: sort by outer_local index so env slot is deterministic.
    let mut outer_vars: Vec<(Rc<str>, Local)> = ctx
        .variable_map
        .iter()
        .filter(|(name, _)| {
            !param_names.contains(name.as_ref()) && closure.self_name != Some(name.as_ref())
        })
        .map(|(name, &local)| (name.clone(), local))
        .collect();
    outer_vars.sort_by_key(|(_, local)| local.0);

    outer_vars
        .into_iter()
        .map(|(name, outer_local)| {
            let cap_ty = capture_type(ctx, outer_local);
            let lambda_local = lambda_ctx.push_param(name.to_string(), cap_ty, closure.span);
            (name, outer_local, lambda_local)
        })
        .collect()
}

/// The type a capture of `outer_local` holds. A nested function's pointer to
/// its own closure is captured as the closure value it is, not as a raw pointer.
fn capture_type(ctx: &LoweringContext, outer_local: Local) -> Type {
    match ctx.self_references.get(&outer_local) {
        Some(function_ty) => function_ty.clone(),
        None => ctx.body.local_decls[outer_local.0].ty.clone(),
    }
}

/// Keep only the tentative captures whose closure-body local the body
/// references, recording each kept one in `body.env_capture_locals`.
///
/// An unreferenced capture's local stays allocated but is left out of
/// `env_capture_locals` and of the closure aggregate's operands. When the body
/// never writes it either, its storage markers are removed too: the local is
/// never initialized, and a `StorageDead` would have Perceus release whatever
/// the uninitialized slot holds.
fn keep_referenced_captures(
    body: &mut Body,
    tentative: &[(Rc<str>, Local, Local)],
) -> Vec<CapturedVar> {
    let referenced_locals = collect_referenced_locals(body);
    let written_locals = body.written_locals();
    let mut captures = Vec::new();
    let mut untouched = HashSet::new();
    for (name, outer_local, lambda_local) in tentative {
        if referenced_locals.contains(lambda_local) {
            body.env_capture_locals.push(*lambda_local);
            captures.push(CapturedVar {
                name: name.clone(),
                lambda_local: *lambda_local,
                outer_local: *outer_local,
            });
        } else if !written_locals.contains(lambda_local) {
            untouched.insert(*lambda_local);
        }
    }
    remove_storage_markers(body, &untouched);
    captures
}

fn remove_storage_markers(body: &mut Body, locals: &HashSet<Local>) {
    if locals.is_empty() {
        return;
    }
    for block in &mut body.basic_blocks {
        block.statements.retain(|stmt| {
            if let MirStatementKind::StorageLive(place) | MirStatementKind::StorageDead(place) =
                &stmt.kind
            {
                !locals.contains(&place.local)
            } else {
                true
            }
        });
    }
}

/// Allocate the closure struct over `captures` at the creation site and store
/// it in `dest` (a fresh temporary when `None`).
fn emit_closure_aggregate(
    ctx: &mut LoweringContext,
    closure: &ClosureSource,
    captures: &[CapturedVar],
    dest: Option<Place>,
) -> Operand {
    // Build capture operands from the outer scope's locals. A self-reference
    // becomes a counted closure value first, released once the aggregate has
    // taken its own reference.
    let watermark = ctx.body.local_decls.len();
    let capture_operands: Vec<Operand> = captures
        .iter()
        .map(|cap| {
            if ctx.self_references.contains_key(&cap.outer_local) {
                lower_self_reference_value(ctx, cap.outer_local, closure.span, None)
            } else {
                Operand::Copy(Place::new(cap.outer_local))
            }
        })
        .collect();

    // Record capture types in the outer body for Perceus / codegen.
    // Stored as AST Type so both Perceus (via MirType::from_type_kind) and
    // codegen (via TypeKind) can use the same source of truth.
    let capture_ast_types: Vec<Type> = captures
        .iter()
        .map(|cap| capture_type(ctx, cap.outer_local))
        .collect();
    let operand_locals: Vec<Local> = capture_operands
        .iter()
        .filter_map(|op| match op {
            Operand::Copy(place) => Some(place.local),
            Operand::Move(_) | Operand::Constant(_) => None,
        })
        .collect();

    let target =
        dest.unwrap_or_else(|| Place::new(ctx.push_temp(closure.ty.clone(), closure.span)));

    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(
            target.clone(),
            Rvalue::Aggregate(
                AggregateKind::Closure(closure.symbol.link_name().into(), closure.ty.clone()),
                capture_operands,
            ),
        ),
        span: closure.span,
    });

    // Register the capture types against the closure local so Perceus and
    // codegen can emit / translate DecRef(closure.field(i)) correctly.
    // Always update (insert or remove) so that re-assigning a `var` closure
    // to a capture-free closure clears any stale entry from the previous
    // assignment — a stale entry would cause a spurious DecRef of the new
    // closure's payload at StorageDead.
    if capture_ast_types.is_empty() {
        ctx.body.closure_capture_types.remove(&target.local);
    } else {
        ctx.body
            .closure_capture_types
            .insert(target.local, capture_ast_types);
    }

    for local in operand_locals {
        ctx.emit_temp_drop(local, watermark, closure.span);
    }

    Operand::Copy(target)
}
