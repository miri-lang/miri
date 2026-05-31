// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::TypeKind;
use miri::codegen::cranelift::translator::TypeCtx;
use miri::codegen::cranelift::FunctionTranslator;
use miri::mir::{Local, Operand};

use miri::ast::types::Type;
use miri::error::syntax::Span;
use miri::mir::Place;
use std::collections::HashMap;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn local_types_for(kinds: &[TypeKind]) -> Vec<Type> {
    kinds.iter().map(|k| ty(k.clone())).collect()
}

fn type_ctx_with<'a>(local_types: &'a [&'a Type]) -> TypeCtx<'a> {
    static EMPTY_DEFS: std::sync::OnceLock<
        HashMap<String, miri::type_checker::context::TypeDefinition>,
    > = std::sync::OnceLock::new();
    static EMPTY_CAPS: std::sync::OnceLock<HashMap<Local, Vec<Type>>> = std::sync::OnceLock::new();
    static EMPTY_OUT: std::sync::OnceLock<HashMap<Local, cranelift_frontend::Variable>> =
        std::sync::OnceLock::new();
    TypeCtx {
        local_types,
        type_definitions: EMPTY_DEFS.get_or_init(HashMap::new),
        ptr_type: cranelift_codegen::ir::types::I64,
        closure_capture_ast_types: EMPTY_CAPS.get_or_init(HashMap::new),
        out_param_ptr_vars: EMPTY_OUT.get_or_init(HashMap::new),
    }
}

#[test]
fn scalar_out_local_returns_local_for_scalar_out_arg() {
    let locals_ty = local_types_for(&[TypeKind::Int]);
    let refs: Vec<&Type> = locals_ty.iter().collect();
    let type_ctx = type_ctx_with(&refs);
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: Vec::new(),
    });
    let got = FunctionTranslator::scalar_out_local_for_arg(&[true], 0, &arg, &type_ctx);
    assert_eq!(got, Some(Local(0)));
}

#[test]
fn scalar_out_local_skips_managed_args() {
    // Managed types (e.g. String) do not need an out pointer — they're already pointers.
    let locals_ty = local_types_for(&[TypeKind::String]);
    let refs: Vec<&Type> = locals_ty.iter().collect();
    let type_ctx = type_ctx_with(&refs);
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: Vec::new(),
    });
    let got = FunctionTranslator::scalar_out_local_for_arg(&[true], 0, &arg, &type_ctx);
    assert!(got.is_none());
}

#[test]
fn scalar_out_local_returns_none_when_flag_unset() {
    let locals_ty = local_types_for(&[TypeKind::Int]);
    let refs: Vec<&Type> = locals_ty.iter().collect();
    let type_ctx = type_ctx_with(&refs);
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: Vec::new(),
    });
    let got = FunctionTranslator::scalar_out_local_for_arg(&[false], 0, &arg, &type_ctx);
    assert!(got.is_none());
}

#[test]
fn scalar_out_local_skips_projected_places() {
    let locals_ty = local_types_for(&[TypeKind::Int]);
    let refs: Vec<&Type> = locals_ty.iter().collect();
    let type_ctx = type_ctx_with(&refs);
    let arg = Operand::Copy(Place {
        local: Local(0),
        projection: vec![miri::mir::PlaceElem::Field(0)],
    });
    let got = FunctionTranslator::scalar_out_local_for_arg(&[true], 0, &arg, &type_ctx);
    assert!(got.is_none());
}
