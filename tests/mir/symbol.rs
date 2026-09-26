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
fn a_function_without_arguments_is_its_name_under_the_root() {
    assert_eq!(Symbol::function("pick", &[]).link_name(), "miri.pick");
}

#[test]
fn a_generic_function_appends_each_argument_token() {
    let symbol = Symbol::function("pick", &[ty(TypeKind::Int), ty(TypeKind::String)]);
    assert_eq!(symbol.link_name(), "miri.pick$int$String");
}

#[test]
fn a_method_is_a_segment_of_its_owner() {
    assert_eq!(
        Symbol::method("Point", &[], "norm", &[]).link_name(),
        "miri.Point.norm"
    );
}

#[test]
fn owner_and_method_arguments_follow_their_own_names() {
    let symbol = Symbol::method(
        "Base",
        &[list_of(TypeKind::String)],
        "map",
        &[ty(TypeKind::Int)],
    );
    assert_eq!(symbol.link_name(), "miri.Base$List_String.map$int");
}

#[test]
fn a_vtable_is_a_marker_segment_of_its_instantiation() {
    assert_eq!(
        Symbol::vtable("Shape", &[]).link_name(),
        "miri.Shape.$vtable"
    );
    assert_eq!(
        Symbol::vtable("Box", &[ty(TypeKind::Int)]).link_name(),
        "miri.Box$int.$vtable"
    );
}

#[test]
fn a_lambda_is_named_by_its_expression_id_and_context() {
    assert_eq!(
        Symbol::closure(ClosureKind::Lambda, 42, &[]).link_name(),
        "miri.$lambda42"
    );
    assert_eq!(
        Symbol::closure(ClosureKind::Lambda, 42, &[ty(TypeKind::Int)]).link_name(),
        "miri.$lambda42$int"
    );
}

#[test]
fn a_function_reference_thunk_names_its_target() {
    let kind = ClosureKind::FunctionReference("miri.double".to_string());
    assert_eq!(
        Symbol::closure(kind, 7, &[]).link_name(),
        "miri.$fnref7.miri.double"
    );
}

#[test]
fn a_nested_function_names_its_declaration() {
    let kind = ClosureKind::NestedFunction("helper".to_string());
    assert_eq!(
        Symbol::closure(kind, 9, &[ty(TypeKind::String)]).link_name(),
        "miri.$nested9$String.helper"
    );
}

#[test]
fn a_residency_segment_lists_each_position_and_handle() {
    let symbol = Symbol::function("scale", &[])
        .with_residency(&[(0, DeviceHandleId(1)), (2, DeviceHandleId(5))]);
    assert_eq!(symbol.link_name(), "miri.$gpu$p0h1$p2h5.scale");
}

#[test]
fn an_empty_residency_adds_no_segment() {
    let symbol = Symbol::closure(ClosureKind::Lambda, 3, &[]).with_residency(&[]);
    assert_eq!(symbol.link_name(), "miri.$lambda3");
}

#[test]
fn a_residency_segment_precedes_the_closure_it_specializes() {
    let symbol = Symbol::closure(ClosureKind::Lambda, 3, &[ty(TypeKind::Int)])
        .with_residency(&[(1, DeviceHandleId(2))]);
    assert_eq!(symbol.link_name(), "miri.$gpu$p1h2.$lambda3$int");
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
fn a_wgsl_name_joins_the_parts_of_a_symbol_with_underscores() {
    assert_eq!(
        Symbol::function("pick", &[ty(TypeKind::Int)]).wgsl_name(),
        "pick__int"
    );
    assert_eq!(Symbol::function("soup", &[]).wgsl_name(), "soup");
    assert_eq!(
        Symbol::method(
            "Base",
            &[list_of(TypeKind::String)],
            "map",
            &[ty(TypeKind::Int)]
        )
        .wgsl_name(),
        "Base_map__List_String__int"
    );
    assert_eq!(
        Symbol::function("scale", &[])
            .with_residency(&[(0, DeviceHandleId(1)), (2, DeviceHandleId(5))])
            .wgsl_name(),
        "scale__gpu_p0h1_p2h5"
    );
}

#[test]
fn a_declared_function_named_main_is_the_entry_point() {
    assert_eq!(Symbol::declared_function("main"), Symbol::entry());
    assert_eq!(
        Symbol::declared_function("pick"),
        Symbol::function("pick", &[])
    );
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
fn symbols_whose_identifiers_run_together_spell_distinct_names() {
    let pairs = [
        (
            Symbol::method("A_b", &[], "c", &[]),
            Symbol::method("A", &[], "b_c", &[]),
        ),
        (
            Symbol::function("pick", &[ty(TypeKind::Int)]),
            Symbol::function("pick__int", &[]),
        ),
        (
            Symbol::method("Pick", &[], "_int", &[]),
            Symbol::function("Pick", &[ty(TypeKind::Int)]),
        ),
        (
            Symbol::type_thunk(ThunkKind::Drop, "W", &[ty(TypeKind::Int)]),
            Symbol::type_thunk(ThunkKind::Drop, "W__int", &[]),
        ),
        (
            Symbol::type_thunk(ThunkKind::Drop, "Point", &[]),
            Symbol::function("__drop_Point", &[]),
        ),
    ];
    for (first, second) in pairs {
        assert_ne!(first, second);
        assert_ne!(first.link_name(), second.link_name());
    }
}

#[test]
fn symbols_display_as_their_link_name() {
    let symbol = Symbol::function("pick", &[ty(TypeKind::Int)]);
    assert_eq!(symbol.to_string(), symbol.link_name());
}

#[test]
fn a_type_thunk_is_a_marker_segment_of_the_type_and_its_arguments() {
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Drop, "Box", &[]).link_name(),
        "miri.Box.$drop"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Decref, "Box", &[ty(TypeKind::String)]).link_name(),
        "miri.Box$String.$decref"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Clone, "Point", &[]).link_name(),
        "miri.Point.$clone"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Compare, "Box", &[ty(TypeKind::Int)]).link_name(),
        "miri.Box$int.$compare"
    );
    assert_eq!(
        Symbol::type_thunk(ThunkKind::Equals, "Point", &[]).link_name(),
        "miri.Point.$equals"
    );
}

#[test]
fn a_structural_thunk_ends_in_the_encoding_of_its_structure() {
    assert_eq!(
        Symbol::structural_thunk(ThunkKind::Decref, ".T2_int_String").link_name(),
        "miri.$decref..T2_int_String"
    );
}

#[test]
fn a_closure_destructor_names_the_closure_body() {
    assert_eq!(
        Symbol::closure_destructor("miri.$lambda7").link_name(),
        "miri.$dtor.miri.$lambda7"
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
    // No identifier contains `.`, so only a name built outside the grammar
    // can spell another symbol's name; the table refuses it all the same.
    let existing = Symbol::method("A", &[], "b", &[]);
    let incoming = Symbol::function("A.b", &[]);
    assert_eq!(table.claim(&existing), Ok(Claim::New));
    assert_eq!(
        table.claim(&incoming),
        Err(Box::new(SymbolCollision {
            existing: existing.clone(),
            incoming: incoming.clone(),
            link_name: "miri.A.b".to_string(),
        }))
    );
    assert!(table.is_claimed(&existing));
    assert!(!table.is_claimed(&incoming));
}

#[test]
fn a_collision_names_both_definitions_as_the_source_writes_them() {
    let mut table = SymbolTable::default();
    table
        .claim(&Symbol::function("pick$int", &[]))
        .expect("first claim is new");
    let collision = table
        .claim(&Symbol::function("pick", &[ty(TypeKind::Int)]))
        .expect_err("a second definition of `miri.pick$int` collides");
    assert_eq!(
        collision.to_string(),
        "`pick$int` and `pick<int>` compile to the same symbol `miri.pick$int`"
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

/// Every identifier of up to `max_len` characters drawn from `alphabet`.
fn identifiers(alphabet: &[char], max_len: usize) -> Vec<String> {
    let mut all = Vec::new();
    let mut frontier = vec![String::new()];
    for _ in 0..max_len {
        frontier = frontier
            .iter()
            .flat_map(|prefix| {
                alphabet.iter().map(move |c| {
                    let mut next = prefix.clone();
                    next.push(*c);
                    next
                })
            })
            .collect();
        all.extend(frontier.iter().cloned());
    }
    all.retain(|name| !name.starts_with(|c: char| c.is_ascii_digit()));
    all
}

/// Every symbol a program can name from `names` and `argument_lists`.
fn symbols_named_from(names: &[String], argument_lists: &[Vec<Type>]) -> Vec<Symbol> {
    let mut symbols = vec![Symbol::entry()];
    for name in names {
        symbols.push(Symbol::runtime(name));
        for args in argument_lists {
            symbols.push(Symbol::function(name, args));
            symbols.push(Symbol::vtable(name, args));
            symbols.push(Symbol::type_thunk(ThunkKind::Drop, name, args));
            symbols.push(Symbol::type_thunk(ThunkKind::Decref, name, args));
            symbols.push(Symbol::function(name, args).with_residency(&[(0, DeviceHandleId(1))]));
            symbols.push(Symbol::closure(
                ClosureKind::NestedFunction(name.clone()),
                1,
                args,
            ));
            let target = Symbol::function(name, args).link_name();
            symbols.push(Symbol::closure(
                ClosureKind::FunctionReference(target),
                1,
                &[],
            ));
        }
    }
    for id in [1, 11, 111] {
        for args in argument_lists {
            symbols.push(Symbol::closure(ClosureKind::Lambda, id, args));
        }
    }
    let owners = names.iter().filter(|name| name.len() <= 3);
    for owner in owners {
        for method in names.iter().filter(|name| name.len() <= 3) {
            for args in argument_lists {
                symbols.push(Symbol::method(owner, args, method, &[]));
                symbols.push(Symbol::method(owner, &[], method, args));
            }
        }
    }
    symbols
}

#[test]
fn distinct_symbols_never_spell_one_link_name() {
    let names = identifiers(&['a', '_', 'A', '1'], 4);
    let argument_lists = vec![
        Vec::new(),
        vec![ty(TypeKind::Int)],
        vec![ty(TypeKind::Int), ty(TypeKind::String)],
        vec![list_of(TypeKind::String)],
        vec![ty(TypeKind::Custom("a_A".to_string(), None))],
    ];
    let symbols = symbols_named_from(&names, &argument_lists);
    let mut seen: std::collections::HashMap<String, Symbol> = std::collections::HashMap::new();
    for symbol in symbols {
        let link_name = symbol.link_name();
        if let Some(earlier) = seen.get(&link_name) {
            assert_eq!(earlier, &symbol, "two symbols spell `{link_name}`");
        }
        let is_foreign = symbol.is_runtime() || symbol == Symbol::entry();
        assert_eq!(
            link_name.contains('.'),
            !is_foreign,
            "`{link_name}`: only a runtime or entry symbol may be spelled without a `.`"
        );
        seen.insert(link_name, symbol);
    }
}
