// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement lowering - converts AST statements to MIR.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::declaration::{
    ClassDecl, Declaration, EnumDecl, FieldDecl, MethodDecl, StructDecl, TraitDecl, TypeAliasDecl,
    VariantDecl,
};
use crate::mir::module::{Import, ImportItem, ImportKind, ImportSource};
use crate::mir::types::MirType;
use crate::mir::{
    Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator, TerminatorKind,
};
use crate::type_checker::context::{GenericDefinition, TypeDefinition};

use super::context::LoweringContext;
use super::control_flow::{lower_break, lower_continue, lower_for, lower_if, lower_while};
use super::expression::lower_expression;
use super::helpers::{coerce_rvalue, mir_types_structurally_match, resolve_type};
use super::variable::lower_variable;

/// Lower an AST statement to MIR.
///
/// Dispatches on the statement kind and delegates to specialized lowering functions
/// for control flow, variable declarations, type declarations, and imports.
///
/// # Errors
///
/// Returns `LoweringError` if any sub-expression or sub-statement fails to lower.
pub fn lower_statement(ctx: &mut LoweringContext, stmt: &Statement) -> Result<(), LoweringError> {
    match &stmt.node {
        StatementKind::Block(stmts) => lower_block(ctx, stmts, stmt.span),
        StatementKind::Return(ret_expr) => lower_return(ctx, ret_expr.as_deref(), stmt.span),
        StatementKind::Variable(decls, _) => lower_variable(ctx, decls, &stmt.span),
        StatementKind::Expression(expr) => lower_expression_stmt(ctx, expr),
        StatementKind::If(cond, then_block, else_block_opt, if_type) => {
            lower_if(ctx, &stmt.span, cond, then_block, else_block_opt, if_type)
        }
        StatementKind::Break => lower_break(ctx, &stmt.span),
        StatementKind::Continue => lower_continue(ctx, &stmt.span),
        StatementKind::While(cond, body, while_type) => {
            lower_while(ctx, &stmt.span, cond, body, while_type)
        }
        StatementKind::For(decls, iterable, body) => {
            lower_for(ctx, &stmt.span, decls, iterable, body)
        }
        StatementKind::GpuFor(decls, iterable, body) => {
            super::gpu_for::lower_gpu_for(ctx, &stmt.span, stmt.id, decls, iterable, body)
        }
        StatementKind::GpuFrame(decls, iterable, body) => {
            super::gpu_frame::lower_gpu_frame(ctx, &stmt.span, stmt.id, decls, iterable, body)
        }
        StatementKind::GpuFrameBlock(block) => {
            super::gpu_frame::lower_gpu_frame_block(ctx, &stmt.span, stmt.id, block)
        }
        StatementKind::Struct(name_expr, _, _, _, _) => {
            lower_struct_decl(ctx, name_expr);
            Ok(())
        }
        StatementKind::Enum(name_expr, _, _, _, _, _) => {
            lower_enum_decl(ctx, name_expr);
            Ok(())
        }
        StatementKind::Class(class_data) => {
            lower_class_decl(ctx, &class_data.name);
            Ok(())
        }
        StatementKind::Trait(name_expr, _, _, _, _) => {
            lower_trait_decl(ctx, name_expr);
            Ok(())
        }
        StatementKind::Type(decls, _) => {
            lower_type_alias(ctx, decls);
            Ok(())
        }
        StatementKind::Use(import_path_expr, alias_opt) => {
            lower_use_stmt(ctx, import_path_expr, alias_opt.as_deref());
            Ok(())
        }
        StatementKind::Empty => Ok(()),
        StatementKind::FunctionDeclaration(decl) => {
            lower_nested_function_decl(ctx, decl, stmt.span)
        }
        StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => Ok(()),
    }
}

fn lower_block(
    ctx: &mut LoweringContext,
    stmts: &[Statement],
    span: Span,
) -> Result<(), LoweringError> {
    ctx.push_scope();
    for s in stmts {
        lower_statement(ctx, s)?;
    }
    ctx.pop_scope(span);
    Ok(())
}

fn lower_return(
    ctx: &mut LoweringContext,
    ret_expr: Option<&Expression>,
    span: Span,
) -> Result<(), LoweringError> {
    if let Some(expr) = ret_expr {
        let ret_ty = ctx.body.local_decls[0].ty.clone();
        let types_match = ctx
            .type_checker
            .get_type(expr.id)
            .map(|ety| {
                let em = MirType::from_type_kind(&ety.kind);
                let rm = MirType::from_type_kind(&ret_ty.kind);
                em == rm || mir_types_structurally_match(&em, &rm)
            })
            .unwrap_or(false);

        if types_match {
            lower_expression(ctx, expr, Some(Place::new(crate::mir::Local(0))))?;
        } else {
            let ret_val = lower_expression(ctx, expr, None)?;
            let val_ty = ret_val.ty(&ctx.body).clone();
            let rvalue = if val_ty.kind != ret_ty.kind {
                coerce_rvalue(ret_val, &val_ty, &ret_ty)
            } else {
                Rvalue::Use(ret_val)
            };
            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(crate::mir::Local(0)), rvalue),
                span,
            });
        }
    }
    // Emit StorageDead for all live named locals before returning so Perceus
    // can insert DecRef for any managed values still in scope (early-return).
    ctx.emit_return_cleanup(span);
    ctx.set_terminator(Terminator::new(TerminatorKind::Return, span));
    Ok(())
}

fn lower_expression_stmt(
    ctx: &mut LoweringContext,
    expr: &Expression,
) -> Result<(), LoweringError> {
    let rhs_watermark = ctx.body.local_decls.len();
    let operand = lower_expression(ctx, expr, None)?;

    // `operand.ty()` returns the base local's type and ignores projections.
    // For projected places like `a[0]` (returns Array instead of element type)
    // fall back to the type checker. For constants / plain locals use the
    // operand type directly to avoid the type checker returning the LHS type
    // (e.g. `int?` for `x = 20` when x is `int?`).
    let ty = match &operand {
        Operand::Constant(c) => c.ty.clone(),
        Operand::Copy(place) | Operand::Move(place) => {
            if !place.projection.is_empty() {
                ctx.type_checker
                    .get_type(expr.id)
                    .cloned()
                    .unwrap_or_else(|| ctx.body.local_decls[place.local.0].ty.clone())
            } else {
                ctx.body.local_decls[place.local.0].ty.clone()
            }
        }
    };

    let temp = ctx.push_temp(ty, expr.span);
    ctx.push_statement(crate::mir::Statement {
        kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(operand.clone())),
        span: expr.span,
    });
    ctx.emit_temp_drop(temp, 0, expr.span);

    if let Operand::Copy(place) | Operand::Move(place) = &operand {
        if place.local != temp {
            ctx.emit_temp_drop(place.local, rhs_watermark, expr.span);
        }
    }
    Ok(())
}

fn extract_identifier(expr: &Expression) -> Option<&str> {
    if let ExpressionKind::Identifier(name, _) = &expr.node {
        Some(name)
    } else {
        None
    }
}

fn collect_generics(generics: Option<&Vec<GenericDefinition>>) -> Vec<String> {
    generics
        .map(|gs| gs.iter().map(|g| g.name.clone()).collect())
        .unwrap_or_default()
}

fn lower_struct_decl(ctx: &mut LoweringContext, name_expr: &Expression) {
    let Some(name) = extract_identifier(name_expr) else {
        return;
    };
    let Some(TypeDefinition::Struct(def)) = ctx.type_checker.global_type_definitions.get(name)
    else {
        return;
    };
    let fields = def
        .fields
        .iter()
        .enumerate()
        .map(|(idx, (field_name, ty, vis))| FieldDecl {
            name: field_name.clone(),
            ty: ty.clone(),
            visibility: vis.clone(),
            index: idx,
            mutable: false,
        })
        .collect();
    let generics = collect_generics(def.generics.as_ref());
    ctx.declarations.push(Declaration::Struct(StructDecl {
        name: name.to_string(),
        fields,
        generics,
        module: def.module.clone(),
    }));
}

fn lower_enum_decl(ctx: &mut LoweringContext, name_expr: &Expression) {
    let Some(name) = extract_identifier(name_expr) else {
        return;
    };
    let Some(TypeDefinition::Enum(def)) = ctx.type_checker.global_type_definitions.get(name) else {
        return;
    };
    let variants = def
        .variants
        .iter()
        .enumerate()
        .map(|(idx, (variant_name, associated_types))| VariantDecl {
            name: variant_name.clone(),
            fields: associated_types.clone(),
            discriminant: idx,
        })
        .collect();
    let generics = collect_generics(def.generics.as_ref());
    ctx.declarations.push(Declaration::Enum(EnumDecl {
        name: name.to_string(),
        variants,
        generics,
        module: ctx.type_checker.current_module.clone(),
    }));
}

fn lower_class_decl(ctx: &mut LoweringContext, name_expr: &Expression) {
    let Some(name) = extract_identifier(name_expr) else {
        return;
    };
    let Some(TypeDefinition::Class(def)) = ctx.type_checker.global_type_definitions.get(name)
    else {
        return;
    };
    let fields = build_field_decls(def);
    let methods = build_method_decls(def);
    let generics = collect_generics(def.generics.as_ref());
    ctx.declarations.push(Declaration::Class(ClassDecl {
        name: name.to_string(),
        fields,
        methods,
        generics,
        base_class: def.base_class.clone(),
        traits: def.traits.clone(),
        module: def.module.clone(),
    }));
}

/// Build the lowered field declarations for a class.
fn build_field_decls(def: &crate::type_checker::context::ClassDefinition) -> Vec<FieldDecl> {
    def.fields
        .iter()
        .enumerate()
        .map(|(idx, (field_name, field_info))| FieldDecl {
            name: field_name.clone(),
            ty: field_info.ty.clone(),
            visibility: field_info.visibility.clone(),
            index: idx,
            mutable: field_info.mutable,
        })
        .collect()
}

/// Build the lowered method declarations for a class.
fn build_method_decls(def: &crate::type_checker::context::ClassDefinition) -> Vec<MethodDecl> {
    def.methods
        .iter()
        .map(|(method_name, method_info)| MethodDecl {
            name: method_name.clone(),
            params: method_info.params.clone(),
            return_type: method_info.return_type.clone(),
            visibility: method_info.visibility.clone(),
            is_constructor: method_info.is_constructor,
        })
        .collect()
}

fn lower_trait_decl(ctx: &mut LoweringContext, name_expr: &Expression) {
    let Some(name) = extract_identifier(name_expr) else {
        return;
    };
    let Some(TypeDefinition::Trait(def)) = ctx.type_checker.global_type_definitions.get(name)
    else {
        return;
    };
    let methods = def
        .methods
        .iter()
        .map(|(method_name, method_info)| MethodDecl {
            name: method_name.clone(),
            params: method_info.params.clone(),
            return_type: method_info.return_type.clone(),
            visibility: method_info.visibility.clone(),
            is_constructor: method_info.is_constructor,
        })
        .collect();
    let generics = collect_generics(def.generics.as_ref());
    ctx.declarations.push(Declaration::Trait(TraitDecl {
        name: name.to_string(),
        methods,
        generics,
        parent_traits: def.parent_traits.clone(),
        module: def.module.clone(),
    }));
}

fn lower_type_alias(ctx: &mut LoweringContext, decls: &[Expression]) {
    for decl in decls {
        let ExpressionKind::TypeDeclaration(name_expr, _, _, target_type) = &decl.node else {
            continue;
        };
        let Some(name) = extract_identifier(name_expr) else {
            continue;
        };
        let Some(target) = target_type else {
            continue;
        };
        let resolved_ty = resolve_type(ctx.type_checker, target);
        ctx.declarations.push(Declaration::TypeAlias(TypeAliasDecl {
            name: name.to_string(),
            target: resolved_ty,
        }));
    }
}

/// Build the `ImportKind` (all/wildcard vs named list) from an import-path kind.
fn build_import_kind(kind: &crate::ast::expression::ImportPathKind) -> ImportKind {
    match kind {
        crate::ast::expression::ImportPathKind::Simple
        | crate::ast::expression::ImportPathKind::Wildcard => ImportKind::All,
        crate::ast::expression::ImportPathKind::Multi(items) => {
            let import_items: Vec<ImportItem> = items
                .iter()
                .filter_map(|(name_expr, alias_expr)| {
                    let name = extract_identifier(name_expr)?.to_string();
                    let alias = alias_expr
                        .as_ref()
                        .and_then(|a| extract_identifier(a).map(|s| s.to_string()));
                    Some(ImportItem { name, alias })
                })
                .collect();
            ImportKind::Named(import_items)
        }
    }
}

fn lower_use_stmt(
    ctx: &mut LoweringContext,
    import_path_expr: &Expression,
    alias_opt: Option<&Expression>,
) {
    let ExpressionKind::ImportPath(segments, kind) = &import_path_expr.node else {
        return;
    };
    let path_strs: Vec<String> = segments
        .iter()
        .filter_map(|seg| extract_identifier(seg).map(|s| s.to_string()))
        .collect();

    if path_strs.is_empty() {
        return;
    }

    let (source, module_path) = match path_strs[0].as_str() {
        "system" => (ImportSource::System, path_strs[1..].to_vec()),
        "local" => (ImportSource::Local, path_strs[1..].to_vec()),
        package_name => (
            ImportSource::Package(package_name.to_string()),
            path_strs[1..].to_vec(),
        ),
    };

    let import_kind = build_import_kind(kind);
    let mut import = Import::new(source, module_path, import_kind);
    if let Some(alias_box) = alias_opt {
        if let Some(alias_name) = extract_identifier(alias_box) {
            import = import.with_alias(alias_name.to_string());
        }
    }
    ctx.imports.push(import);
}

fn lower_nested_function_decl(
    ctx: &mut LoweringContext,
    decl: &crate::ast::statement::FunctionDeclarationData,
    span: Span,
) -> Result<(), LoweringError> {
    use crate::mir::lambda::LambdaInfo;
    use crate::mir::{Body, ExecutionModel, LocalDecl};

    let name = &decl.name;
    let params = &decl.params;
    let ret_type_expr = &decl.return_type;
    let body_stmt = &decl.body;
    let props = &decl.properties;

    let execution_model = if props.is_gpu {
        ExecutionModel::GpuKernel
    } else if props.is_async {
        ExecutionModel::Async
    } else {
        ExecutionModel::Cpu
    };

    let ret_ty = if let Some(ret_expr) = ret_type_expr {
        super::resolve_type(ctx.type_checker, ret_expr)
    } else {
        Type::new(TypeKind::Void, span)
    };

    let mut nested_body = Body::new(params.len(), span, execution_model);
    nested_body.new_local(LocalDecl::new(ret_ty.clone(), span));

    let mut nested_ctx = super::LoweringContext::new(nested_body, ctx.type_checker, ctx.is_release);
    for param in params.iter() {
        let param_ty = super::resolve_type(ctx.type_checker, &param.typ);
        nested_ctx.push_local(param.name.clone(), param_ty, param.typ.span);
    }

    if let Some(body_box) = body_stmt {
        super::lower_as_return(&mut nested_ctx, body_box, &ret_ty)?;
    }

    if nested_ctx.body.basic_blocks[nested_ctx.current_block.0]
        .terminator
        .is_none()
    {
        nested_ctx.set_terminator(crate::mir::Terminator::new(
            crate::mir::TerminatorKind::Return,
            span,
        ));
    }

    ctx.lambda_bodies.push(LambdaInfo {
        name: name.clone(),
        body: nested_ctx.body,
        captures: vec![],
    });

    let func_ty = Type::new(
        TypeKind::Function(Box::new(crate::ast::types::FunctionTypeData {
            generics: None,
            params: params
                .iter()
                .map(|p| crate::ast::common::Parameter {
                    name: p.name.clone(),
                    typ: p.typ.clone(),
                    guard: p.guard.clone(),
                    default_value: p.default_value.clone(),
                    is_out: p.is_out,
                })
                .collect(),
            return_type: ret_type_expr.clone(),
        })),
        span,
    );
    ctx.push_local(name.clone(), func_ty, span);
    Ok(())
}
