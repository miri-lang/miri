// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement lowering - converts AST statements to MIR.

use crate::ast::expression::ExpressionKind;
use crate::ast::statement::{Statement, StatementKind};
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::mir::declaration::{
    ClassDecl, Declaration, EnumDecl, FieldDecl, MethodDecl, StructDecl, TraitDecl, TypeAliasDecl,
    VariantDecl,
};
use crate::mir::module::{Import, ImportItem, ImportKind, ImportSource};
use crate::mir::{
    Operand, Place, Rvalue, StatementKind as MirStatementKind, Terminator, TerminatorKind,
};
use crate::type_checker::context::TypeDefinition;

use super::context::LoweringContext;
use super::control_flow::{lower_break, lower_continue, lower_for, lower_if, lower_while};
use super::expression::lower_expression;
use super::helpers::resolve_type;
use super::variable::lower_variable;

pub fn lower_statement(ctx: &mut LoweringContext, stmt: &Statement) -> Result<(), LoweringError> {
    match &stmt.node {
        StatementKind::Block(stmts) => {
            // A block defines a new scope. Variables declared within
            // will be tracked and removed when the block ends.
            ctx.push_scope();
            for s in stmts {
                lower_statement(ctx, s)?;
            }
            ctx.pop_scope(stmt.span.clone());
        }
        StatementKind::Return(ret_expr) => {
            if let Some(expr) = ret_expr {
                let ret_ty = ctx.body.local_decls[0].ty.clone();
                let expr_ty_opt = ctx.type_checker.get_type(expr.id);
                let types_match = if let Some(ety) = expr_ty_opt {
                    ety.kind == ret_ty.kind
                } else {
                    false
                };

                if types_match {
                    // DPS: Write directly to _0
                    lower_expression(ctx, expr, Some(Place::new(crate::mir::Local(0))))?;
                } else {
                    let ret_val = lower_expression(ctx, expr, None)?;
                    let val_ty = ret_val.ty(&ctx.body);

                    let rvalue = if val_ty.kind != ret_ty.kind {
                        Rvalue::Cast(Box::new(ret_val), ret_ty)
                    } else {
                        Rvalue::Use(ret_val)
                    };

                    ctx.push_statement(crate::mir::Statement {
                        kind: MirStatementKind::Assign(
                            Place::new(crate::mir::Local(0)), // _0 is the return place
                            rvalue,
                        ),
                        span: stmt.span.clone(),
                    });
                }
            }
            ctx.set_terminator(Terminator::new(TerminatorKind::Return, stmt.span.clone()));
        }
        StatementKind::Variable(decls, _) => {
            lower_variable(ctx, decls, &stmt.span)?;
        }
        StatementKind::Expression(expr) => {
            let operand = lower_expression(ctx, expr, None)?;

            let ty = match &operand {
                Operand::Constant(c) => c.ty.clone(),
                Operand::Copy(place) | Operand::Move(place) => {
                    ctx.body.local_decls[place.local.0].ty.clone()
                }
            };

            let temp = ctx.push_temp(ty, expr.span.clone());

            ctx.push_statement(crate::mir::Statement {
                kind: MirStatementKind::Assign(Place::new(temp), Rvalue::Use(operand)),
                span: expr.span.clone(),
            });
        }
        StatementKind::If(cond, then_block, else_block_opt, if_type) => {
            lower_if(ctx, &stmt.span, cond, then_block, else_block_opt, if_type)?;
        }
        StatementKind::Break => {
            lower_break(ctx, &stmt.span)?;
        }
        StatementKind::Continue => {
            lower_continue(ctx, &stmt.span)?;
        }
        StatementKind::While(cond, body, while_type) => {
            lower_while(ctx, &stmt.span, cond, body, while_type)?;
        }
        StatementKind::For(decls, iterable, body) => {
            lower_for(ctx, &stmt.span, decls, iterable, body)?;
        }
        StatementKind::Struct(name_expr, _generics, _members, _vis) => {
            // Lower struct declaration by looking up the type definition from type checker
            if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                if let Some(TypeDefinition::Struct(def)) =
                    ctx.type_checker.global_type_definitions.get(name)
                {
                    let fields = def
                        .fields
                        .iter()
                        .enumerate()
                        .map(|(idx, (field_name, ty, vis))| FieldDecl {
                            name: field_name.clone(),
                            ty: ty.clone(),
                            visibility: vis.clone(),
                            index: idx,
                            mutable: false, // Struct fields are immutable by default
                        })
                        .collect();

                    let generics = def
                        .generics
                        .as_ref()
                        .map(|gs| gs.iter().map(|g| g.name.clone()).collect())
                        .unwrap_or_default();

                    ctx.declarations.push(Declaration::Struct(StructDecl {
                        name: name.clone(),
                        fields,
                        generics,
                        module: def.module.clone(),
                    }));
                }
            }
        }
        StatementKind::Enum(name_expr, _generics, _variants, _vis) => {
            // Lower enum declaration by looking up the type definition from type checker
            if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                if let Some(TypeDefinition::Enum(def)) =
                    ctx.type_checker.global_type_definitions.get(name)
                {
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

                    let generics = def
                        .generics
                        .as_ref()
                        .map(|gs| gs.iter().map(|g| g.name.clone()).collect())
                        .unwrap_or_default();

                    ctx.declarations.push(Declaration::Enum(EnumDecl {
                        name: name.clone(),
                        variants,
                        generics,
                        module: def.module.clone(),
                    }));
                }
            }
        }
        StatementKind::Class(
            name_expr,
            _generics,
            _base_class,
            _traits,
            _body,
            _vis,
            _is_abstract,
        ) => {
            // Lower class declaration by looking up the type definition from type checker
            if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                if let Some(TypeDefinition::Class(def)) =
                    ctx.type_checker.global_type_definitions.get(name)
                {
                    let fields = def
                        .fields
                        .iter()
                        .enumerate()
                        .map(|(idx, (field_name, field_info))| FieldDecl {
                            name: field_name.clone(),
                            ty: field_info.ty.clone(),
                            visibility: field_info.visibility.clone(),
                            index: idx,
                            mutable: field_info.mutable,
                        })
                        .collect();

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

                    let generics = def
                        .generics
                        .as_ref()
                        .map(|gs| gs.iter().map(|g| g.name.clone()).collect())
                        .unwrap_or_default();

                    ctx.declarations.push(Declaration::Class(ClassDecl {
                        name: name.clone(),
                        fields,
                        methods,
                        generics,
                        base_class: def.base_class.clone(),
                        traits: def.traits.clone(),
                        module: def.module.clone(),
                    }));
                }
            }
        }
        StatementKind::Trait(name_expr, _generics, _parent_traits, _body, _vis) => {
            // Lower trait declaration by looking up the type definition from type checker
            if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                if let Some(TypeDefinition::Trait(def)) =
                    ctx.type_checker.global_type_definitions.get(name)
                {
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

                    let generics = def
                        .generics
                        .as_ref()
                        .map(|gs| gs.iter().map(|g| g.name.clone()).collect())
                        .unwrap_or_default();

                    ctx.declarations.push(Declaration::Trait(TraitDecl {
                        name: name.clone(),
                        methods,
                        generics,
                        parent_traits: def.parent_traits.clone(),
                        module: def.module.clone(),
                    }));
                }
            }
        }
        StatementKind::Type(decls, _vis) => {
            // Lower type alias declarations
            for decl in decls {
                if let ExpressionKind::TypeDeclaration(name_expr, _generics, _kind, target_type) =
                    &decl.node
                {
                    if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                        if let Some(target) = target_type {
                            let resolved_ty = resolve_type(ctx.type_checker, target);
                            ctx.declarations.push(Declaration::TypeAlias(TypeAliasDecl {
                                name: name.clone(),
                                target: resolved_ty,
                            }));
                        }
                    }
                }
            }
        }
        StatementKind::Use(import_path_expr, alias_opt) => {
            // Lower use/import statements
            if let ExpressionKind::ImportPath(segments, kind) = &import_path_expr.node {
                // Extract path segments as strings
                let path_strs: Vec<String> = segments
                    .iter()
                    .filter_map(|seg| {
                        if let ExpressionKind::Identifier(name, _) = &seg.node {
                            Some(name.clone())
                        } else {
                            None
                        }
                    })
                    .collect();

                if path_strs.is_empty() {
                    return Ok(());
                }

                // Determine import source from first segment
                let (source, module_path) = match path_strs[0].as_str() {
                    "system" => (ImportSource::System, path_strs[1..].to_vec()),
                    "local" => (ImportSource::Local, path_strs[1..].to_vec()),
                    package_name => (
                        ImportSource::Package(package_name.to_string()),
                        path_strs[1..].to_vec(),
                    ),
                };

                // Determine import kind
                let import_kind = match kind {
                    crate::ast::expression::ImportPathKind::Simple => ImportKind::All,
                    crate::ast::expression::ImportPathKind::Wildcard => ImportKind::All,
                    crate::ast::expression::ImportPathKind::Multi(items) => {
                        let import_items: Vec<ImportItem> = items
                            .iter()
                            .filter_map(|(name_expr, alias_expr)| {
                                if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                                    let alias = alias_expr.as_ref().and_then(|a| {
                                        if let ExpressionKind::Identifier(alias_name, _) = &a.node {
                                            Some(alias_name.clone())
                                        } else {
                                            None
                                        }
                                    });
                                    Some(ImportItem {
                                        name: name.clone(),
                                        alias,
                                    })
                                } else {
                                    None
                                }
                            })
                            .collect();
                        ImportKind::Named(import_items)
                    }
                };

                // Create import
                let mut import = Import::new(source, module_path, import_kind);

                // Handle module alias (e.g., `use system.io as input_output`)
                if let Some(alias_box) = alias_opt {
                    if let ExpressionKind::Identifier(alias_name, _) = &alias_box.node {
                        import = import.with_alias(alias_name.clone());
                    }
                }

                ctx.imports.push(import);
            }
        }
        StatementKind::Empty => {
            // Empty statement - no-op, nothing to do
        }
        StatementKind::FunctionDeclaration(
            name,
            _generics,
            params,
            ret_type_expr,
            body_stmt,
            props,
        ) => {
            // Nested function declarations are lowered to LambdaInfo
            // and stored for later codegen. A local variable is created
            // that references this function.
            use crate::mir::lambda::LambdaInfo;
            use crate::mir::{Body, ExecutionModel, LocalDecl};

            // Determine execution model
            let execution_model = if props.is_gpu {
                ExecutionModel::GpuKernel
            } else if props.is_async {
                ExecutionModel::Async
            } else {
                ExecutionModel::Cpu
            };

            // Resolve return type
            let ret_ty = if let Some(ret_expr) = ret_type_expr {
                super::resolve_type(ctx.type_checker, ret_expr)
            } else {
                Type::new(TypeKind::Void, stmt.span.clone())
            };

            // Create a new body for this nested function
            let mut nested_body = Body::new(params.len(), stmt.span.clone(), execution_model);
            nested_body.new_local(LocalDecl::new(ret_ty.clone(), stmt.span.clone()));

            // Create a temporary context for lowering the nested function
            let mut nested_ctx =
                super::LoweringContext::new(nested_body, ctx.type_checker, ctx.is_release);

            // Lower parameters
            for param in params.iter() {
                let param_ty = super::resolve_type(ctx.type_checker, &param.typ);
                nested_ctx.push_local(param.name.clone(), param_ty, param.typ.span.clone());
            }

            // Lower the body if present
            if let Some(body_box) = body_stmt {
                super::lower_as_return(&mut nested_ctx, body_box, &ret_ty)?;
            }

            // Ensure terminator
            if nested_ctx.body.basic_blocks[nested_ctx.current_block.0]
                .terminator
                .is_none()
            {
                nested_ctx.set_terminator(crate::mir::Terminator::new(
                    crate::mir::TerminatorKind::Return,
                    stmt.span.clone(),
                ));
            }

            // Store as lambda info (using function name)
            let lambda_info = LambdaInfo {
                name: name.clone(),
                body: nested_ctx.body,
                captures: vec![], // Nested functions don't capture by default
            };
            ctx.lambda_bodies.push(lambda_info);

            // Create a local variable for this function reference
            let func_ty = Type::new(
                TypeKind::Function(
                    None,
                    params
                        .iter()
                        .map(|p| crate::ast::common::Parameter {
                            name: p.name.clone(),
                            typ: p.typ.clone(),
                            guard: p.guard.clone(),
                            default_value: p.default_value.clone(),
                        })
                        .collect(),
                    ret_type_expr.clone(),
                ),
                stmt.span.clone(),
            );
            ctx.push_local(name.clone(), func_ty, stmt.span.clone());
        }
    }
    Ok(())
}
