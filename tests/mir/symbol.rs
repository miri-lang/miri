// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::body::DeviceHandleId;
use miri::mir::symbol::{
    Claim, ClosureKind, GpuKernelKind, KernelDatum, StringLiteralPart, Symbol, SymbolCollision,
    SymbolTable, ThunkKind,
};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn list_of(element: TypeKind) -> Type {
    ty(TypeKind::List(Box::new(
        miri::ast::factory::type_expr_non_null(ty(element)),
    )))
}

#[test]
fn a_function_without_arguments_is_its_own_name() {
    assert_eq!(Symbol::function("pick", &[]).link_name(), "pick");
}

#[test]
fn a_generic_function_appends_each_argument_token() {
    let symbol = Symbol::function("pick", &[ty(TypeKind::Int), ty(TypeKind::String)]);
    assert_eq!(symbol.link_name(), "pick__int__String");
}

#[test]
fn a_method_joins_owner_and_method_with_one_underscore() {
    assert_eq!(
        Symbol::method("Point", &[], "norm", &[]).link_name(),
        "Point_norm"
    );
}

#[test]
fn a_method_appends_owner_arguments_then_method_arguments() {
    let symbol = Symbol::method(
        "Base",
        &[list_of(TypeKind::String)],
        "map",
        &[ty(TypeKind::Int)],
    );
    assert_eq!(symbol.link_name(), "Base_map__List_String__int");
}

#[test]
fn a_vtable_is_prefixed_and_mangled_by_its_instantiation() {
    assert_eq!(Symbol::vtable("Shape", &[]).link_name(), "__vtable_Shape");
    assert_eq!(
        Symbol::vtable("Box", &[ty(TypeKind::Int)]).link_name(),
        "__vtable_Box__int"
    );
}

#[test]
fn a_lambda_is_named_by_its_expression_id_and_context() {
    assert_eq!(
        Symbol::closure(ClosureKind::Lambda, 42, &[]).link_name(),
        "__lambda_42"
    );
    assert_eq!(
        Symbol::closure(ClosureKind::Lambda, 42, &[ty(TypeKind::Int)]).link_name(),
        "__lambda_42__int"
    );
}

#[test]
fn a_function_reference_thunk_names_its_target() {
    let kind = ClosureKind::FunctionReference("double".to_string());
    assert_eq!(
        Symbol::closure(kind, 7, &[]).link_name(),
        "__fnref_double_7"
    );
}

#[test]
fn a_nested_function_names_its_declaration() {
    let kind = ClosureKind::NestedFunction("helper".to_string());
    assert_eq!(
        Symbol::closure(kind, 9, &[ty(TypeKind::String)]).link_name(),
        "__nested_helper_9__String"
    );
}

#[test]
fn a_residency_suffix_lists_each_position_and_handle() {
    let symbol = Symbol::function("scale", &[])
        .with_residency(&[(0, DeviceHandleId(1)), (2, DeviceHandleId(5))]);
    assert_eq!(symbol.link_name(), "scale__gpu_p0h1_p2h5");
}

#[test]
fn an_empty_residency_adds_no_suffix() {
    let symbol = Symbol::closure(ClosureKind::Lambda, 3, &[]).with_residency(&[]);
    assert_eq!(symbol.link_name(), "__lambda_3");
}

#[test]
fn a_residency_suffix_follows_the_context_arguments() {
    let symbol = Symbol::closure(ClosureKind::Lambda, 3, &[ty(TypeKind::Int)])
        .with_residency(&[(1, DeviceHandleId(2))]);
    assert_eq!(symbol.link_name(), "__lambda_3__int__gpu_p1h2");
}

#[test]
fn gpu_kernels_are_numbered_per_construct() {
    let forall = Symbol::gpu_kernel(GpuKernelKind::Forall, 0);
    let reduce = Symbol::gpu_kernel(GpuKernelKind::Reduce, 4);
    assert_eq!(forall.link_name(), "miri_gpu_forall_0");
    assert_eq!(reduce.link_name(), "miri_gpu_reduce_4");
}

#[test]
fn a_gpu_kernel_entry_point_is_spelled_as_it_is_linked() {
    let kernel = Symbol::gpu_kernel(GpuKernelKind::Forall, 2);
    assert_eq!(kernel.wgsl_name(), kernel.link_name());
}

#[test]
fn runtime_symbols_and_the_entry_point_are_verbatim() {
    let runtime = Symbol::runtime("miri_rt_list_new");
    assert_eq!(runtime.link_name(), "miri_rt_list_new");
    assert!(runtime.is_runtime());
    assert_eq!(Symbol::entry().link_name(), "main");
    assert!(!Symbol::entry().is_runtime());
    assert!(!Symbol::function("miri_rt_list_new", &[]).is_runtime());
}

#[test]
fn symbols_that_differ_in_structure_are_distinct_values() {
    let composed = Symbol::method("A_b", &[], "c", &[]);
    let other = Symbol::method("A", &[], "b_c", &[]);
    assert_eq!(composed.link_name(), other.link_name());
    assert_ne!(composed, other);
}

#[test]
fn symbols_display_as_their_link_name() {
    let symbol = Symbol::function("pick", &[ty(TypeKind::Int)]);
    assert_eq!(symbol.to_string(), symbol.link_name());
}

#[test]
fn a_type_thunk_prefixes_its_kind_to_the_type_and_its_arguments() {
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Drop, "Box", &[]).link_name(),
        "__drop_Box"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Decref, "Box", &[ty(TypeKind::String)]).link_name(),
        "__decref_Box__String"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Clone, "Point", &[]).link_name(),
        "__clone_Point"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Compare, "Box", &[ty(TypeKind::Int)]).link_name(),
        "__compare_Box__int"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Equals, "Point", &[]).link_name(),
        "__equals_Point"
    );
}

#[test]
fn a_structural_thunk_appends_the_encoding_of_its_structure() {
    assert_eq!(
        Symbol::structural_thunk(ThunkKind::Decref, ".T2_int_String").link_name(),
        "__decref_.T2_int_String"
    );
}

#[test]
fn a_closure_destructor_names_the_closure_body() {
    assert_eq!(
        Symbol::closure_destructor("__lambda_7").link_name(),
        "__dtor___lambda_7"
    );
}

#[test]
fn kernel_data_are_named_by_the_kernel_entry_point() {
    let kernel = Symbol::gpu_kernel(GpuKernelKind::Forall, 0).wgsl_name();
    assert_eq!(
        Symbol::kernel_datum(&kernel, KernelDatum::Wgsl).link_name(),
        "__miri_kernel_miri_gpu_forall_0_wgsl"
    );
    assert_eq!(
        Symbol::kernel_datum(&kernel, KernelDatum::Name).link_name(),
        "__miri_kernel_miri_gpu_forall_0_name"
    );
}

#[test]
fn string_literal_data_are_numbered_per_module() {
    assert_eq!(
        Symbol::string_literal(3, StringLiteralPart::Bytes).link_name(),
        ".miri_str_3_bytes"
    );
    assert_eq!(
        Symbol::string_literal(3, StringLiteralPart::Object).link_name(),
        ".miri_str_3_struct"
    );
}

#[test]
fn thunks_of_different_kinds_or_types_are_distinct_values() {
    let drop_box = Symbol::type_thunk(ThunkKind::Drop, "Box", &[]);
    assert_ne!(drop_box, Symbol::type_thunk(ThunkKind::Decref, "Box", &[]));
    assert_ne!(
        drop_box,
        Symbol::type_thunk(ThunkKind::Drop, "Box", &[ty(TypeKind::Int)])
    );
    assert_ne!(
        Symbol::type_thunk(ThunkKind::Drop, "Box__int", &[]),
        Symbol::type_thunk(ThunkKind::Drop, "Box", &[ty(TypeKind::Int)])
    );
}

#[test]
fn a_frame_pass_kernel_is_numbered_by_its_statement_and_pass() {
    assert_eq!(
        Symbol::gpu_kernel(GpuKernelKind::FramePass { pass: 2 }, 5).link_name(),
        "miri_gpu_for_5_2"
    );
}

#[test]
fn claiming_a_symbol_for_the_first_time_is_new() {
    let mut table = SymbolTable::default();
    assert_eq!(
        table.claim(&Symbol::method("A", &[], "b_c", &[])),
        Ok(Claim::New)
    );
}

#[test]
fn claiming_a_symbol_again_finds_it_already_lowered() {
    let mut table = SymbolTable::default();
    let symbol = Symbol::function("pick", &[ty(TypeKind::Int)]);
    assert_eq!(table.claim(&symbol), Ok(Claim::New));
    assert_eq!(table.claim(&symbol), Ok(Claim::AlreadyLowered));
    assert!(table.is_claimed(&symbol));
}

#[test]
fn a_distinct_symbol_spelling_a_claimed_link_name_collides() {
    let mut table = SymbolTable::default();
    let existing = Symbol::method("A_b", &[], "c", &[]);
    let incoming = Symbol::method("A", &[], "b_c", &[]);
    assert_eq!(table.claim(&existing), Ok(Claim::New));
    assert_eq!(
        table.claim(&incoming),
        Err(Box::new(SymbolCollision {
            existing: existing.clone(),
            incoming: incoming.clone(),
            link_name: "A_b_c".to_string(),
        }))
    );
    assert!(table.is_claimed(&existing));
    assert!(!table.is_claimed(&incoming));
}

#[test]
fn a_collision_names_both_definitions_as_the_source_writes_them() {
    let mut table = SymbolTable::default();
    table
        .claim(&Symbol::function("pick__int", &[]))
        .expect("first claim is new");
    let collision = table
        .claim(&Symbol::function("pick", &[ty(TypeKind::Int)]))
        .expect_err("a second definition of `pick__int` collides");
    assert_eq!(
        collision.to_string(),
        "`pick__int` and `pick<int>` compile to the same symbol `pick__int`"
    );
}

#[test]
fn a_method_symbol_answers_which_method_of_its_owner_it_is() {
    let at_int = [ty(TypeKind::Int)];
    let symbol = Symbol::method("Box", &at_int, "get", &[]);
    assert_eq!(symbol.method_of("Box", &at_int), Some("get"));
    assert_eq!(symbol.method_of("Box", &[]), None);
    assert_eq!(symbol.method_of("Bo", &at_int), None);
    assert_eq!(
        Symbol::function("Box_get", &at_int).method_of("Box", &at_int),
        None
    );
}
