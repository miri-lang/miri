// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::{Type, TypeKind};
use miri::diagnostics::DiagnosticCode;
use miri::error::syntax::Span;
use miri::mir::body::DeviceHandleId;
use miri::mir::symbol::{
    Claim, ClaimRefusal, ClosureKind, GpuKernelKind, KernelDatum, Namespace, StringLiteralPart,
    Symbol, SymbolCollision, SymbolTable, ThunkKind,
};
use miri::type_checker::ModuleId;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

/// The module a `use` loads as `path`.
fn module(path: &str) -> ModuleId {
    ModuleId::Imported(path.split('.').map(str::to_string).collect())
}

fn list_of(element: TypeKind) -> Type {
    ty(TypeKind::List(Box::new(
        miri::ast::factory::type_expr_non_null(ty(element)),
    )))
}

#[test]
fn a_function_without_arguments_is_its_name_under_the_root() {
    assert_eq!(
        Symbol::function(&ModuleId::Program, "pick", &[]).link_name(),
        "miri.pick"
    );
}

#[test]
fn a_generic_function_appends_each_argument_token() {
    let symbol = Symbol::function(
        &ModuleId::Program,
        "pick",
        &[ty(TypeKind::Int), ty(TypeKind::String)],
    );
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
    let symbol = Symbol::function(&ModuleId::Program, "scale", &[])
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
        Symbol::function(&ModuleId::Program, "pick", &[ty(TypeKind::Int)]).wgsl_name(),
        "pick__int"
    );
    assert_eq!(
        Symbol::function(&ModuleId::Program, "soup", &[]).wgsl_name(),
        "soup"
    );
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
        Symbol::function(&ModuleId::Program, "scale", &[])
            .with_residency(&[(0, DeviceHandleId(1)), (2, DeviceHandleId(5))])
            .wgsl_name(),
        "scale__gpu_p0h1_p2h5"
    );
}

#[test]
fn a_declared_function_named_main_is_the_entry_point() {
    assert_eq!(
        Symbol::declared_function(&ModuleId::Program, "main"),
        Symbol::entry()
    );
    assert_eq!(
        Symbol::declared_function(&ModuleId::Program, "pick"),
        Symbol::function(&ModuleId::Program, "pick", &[])
    );
}

#[test]
fn a_module_function_names_its_module_in_the_root() {
    assert_eq!(
        Symbol::function(&module("local.m.a"), "helper", &[]).link_name(),
        "miri$local$m$a.helper"
    );
    assert_eq!(
        Symbol::function(&module("system.math"), "pick", &[ty(TypeKind::Int)]).link_name(),
        "miri$system$math.pick$int"
    );
}

#[test]
fn a_residency_segment_follows_a_module_root() {
    let symbol = Symbol::function(&module("local.gpu"), "scale", &[])
        .with_residency(&[(0, DeviceHandleId(1))]);
    assert_eq!(symbol.link_name(), "miri$local$gpu.$gpu$p0h1.scale");
}

#[test]
fn only_the_program_s_own_main_is_the_entry_point() {
    let module_main = Symbol::declared_function(&module("local.tool"), "main");
    assert_ne!(module_main, Symbol::entry());
    assert_eq!(module_main.link_name(), "miri$local$tool.main");
}

#[test]
fn a_module_function_s_wgsl_name_carries_its_module_path() {
    assert_eq!(
        Symbol::function(&module("local.m.a"), "pick", &[ty(TypeKind::Int)]).wgsl_name(),
        "m__5local1m1a_pick__int"
    );
    assert_eq!(
        Symbol::function(&module("system.math"), "lattice_unit", &[]).wgsl_name(),
        "m__6system4math_lattice_unit"
    );
    assert_eq!(
        Symbol::function(&module("local.m.a"), "scale", &[])
            .with_residency(&[(0, DeviceHandleId(1))])
            .wgsl_name(),
        "m__5local1m1a_scale__gpu_p0h1"
    );
}

#[test]
fn a_program_function_keeps_its_name_as_its_wgsl_name() {
    for name in ["helper", "m_helper", "m_", "main2", "_m__x", "mm__x"] {
        assert_eq!(
            Symbol::function(&ModuleId::Program, name, &[]).wgsl_name(),
            name
        );
    }
}

#[test]
fn a_program_function_named_like_a_module_function_s_wgsl_name_is_escaped() {
    let module_function = Symbol::function(&module("system.math"), "lattice_unit", &[]);
    let program_function = Symbol::function(&ModuleId::Program, &module_function.wgsl_name(), &[]);
    assert_eq!(
        program_function.wgsl_name(),
        "m__0_m__6system4math_lattice_unit"
    );
    assert_ne!(program_function.wgsl_name(), module_function.wgsl_name());
}

/// Every pairing of a declaring module and a name spells its own WGSL name:
/// the program's functions against every module's, names beginning `m__`
/// (escaped or not), and module paths whose identifiers could be regrouped
/// across the boundary with the name if they were not length-prefixed.
#[test]
fn distinct_functions_without_arguments_have_distinct_wgsl_names() {
    let modules = [
        ModuleId::Program,
        module("local.a"),
        module("local.b"),
        module("local.ab"),
        module("local.a.b"),
        module("local.a_b"),
        module("local.a1"),
        module("local.a.b_c"),
        module("local.a.b.c"),
        module("system.math"),
        module("m__0"),
    ];
    let names = [
        "helper",
        "b_helper",
        "c_helper",
        "b_c_helper",
        "1a_helper",
        "m__",
        "m__0_helper",
        "m__5local1a_helper",
        "m__5local1a1b_helper",
        "m__6system4math_lattice_unit",
        "lattice_unit",
    ];
    let mut owners: std::collections::HashMap<String, (usize, &str)> =
        std::collections::HashMap::new();
    for (index, module) in modules.iter().enumerate() {
        for name in names {
            let spelling = Symbol::function(module, name, &[]).wgsl_name();
            assert!(!spelling.starts_with("__"), "{spelling} begins with `__`");
            if let Some(previous) = owners.insert(spelling.clone(), (index, name)) {
                panic!(
                    "{spelling} spells both {previous:?} and {:?}",
                    (index, name)
                );
            }
        }
    }
    assert_eq!(owners.len(), modules.len() * names.len());
}

#[test]
fn a_module_function_is_written_under_its_module_path() {
    let symbol = Symbol::function(&module("local.m.a"), "pick", &[ty(TypeKind::Int)]);
    assert_eq!(symbol.written().to_string(), "`local.m.a.pick<int>`");
    assert_eq!(
        Symbol::function(&ModuleId::Program, "pick", &[])
            .written()
            .to_string(),
        "`pick`"
    );
}

#[test]
fn functions_of_one_name_in_different_modules_are_distinct() {
    let pairs = [
        (
            Symbol::function(&ModuleId::Program, "helper", &[]),
            Symbol::function(&module("local.a"), "helper", &[]),
        ),
        (
            Symbol::function(&module("local.a"), "helper", &[]),
            Symbol::function(&module("local.b"), "helper", &[]),
        ),
        (
            Symbol::function(&module("a"), "b", &[]),
            Symbol::method("a", &[], "b", &[]),
        ),
        (
            Symbol::function(&module("a.b"), "c", &[]),
            Symbol::function(&module("a"), "b", &[]),
        ),
    ];
    for (first, second) in pairs {
        assert_ne!(first, second);
        assert_ne!(first.link_name(), second.link_name());
    }
}

#[test]
fn runtime_symbols_and_the_entry_point_are_verbatim() {
    let runtime = Symbol::runtime("miri_rt_list_new");
    assert_eq!(runtime.link_name(), "miri_rt_list_new");
    assert!(runtime.is_runtime());
    assert_eq!(Symbol::entry().link_name(), "main");
    assert!(!Symbol::entry().is_runtime());
    assert!(!Symbol::function(&ModuleId::Program, "miri_rt_list_new", &[]).is_runtime());
}

#[test]
fn symbols_whose_identifiers_run_together_spell_distinct_names() {
    let pairs = [
        (
            Symbol::method("A_b", &[], "c", &[]),
            Symbol::method("A", &[], "b_c", &[]),
        ),
        (
            Symbol::function(&ModuleId::Program, "pick", &[ty(TypeKind::Int)]),
            Symbol::function(&ModuleId::Program, "pick__int", &[]),
        ),
        (
            Symbol::method("Pick", &[], "_int", &[]),
            Symbol::function(&ModuleId::Program, "Pick", &[ty(TypeKind::Int)]),
        ),
        (
            Symbol::type_thunk(ThunkKind::Drop, "W", &[ty(TypeKind::Int)]),
            Symbol::type_thunk(ThunkKind::Drop, "W__int", &[]),
        ),
        (
            Symbol::type_thunk(ThunkKind::Drop, "Point", &[]),
            Symbol::function(&ModuleId::Program, "__drop_Point", &[]),
        ),
    ];
    for (first, second) in pairs {
        assert_ne!(first, second);
        assert_ne!(first.link_name(), second.link_name());
    }
}

#[test]
fn symbols_display_as_their_link_name() {
    let symbol = Symbol::function(&ModuleId::Program, "pick", &[ty(TypeKind::Int)]);
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
    let symbol = Symbol::function(&ModuleId::Program, "pick", &[ty(TypeKind::Int)]);
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
    let incoming = Symbol::function(&ModuleId::Program, "A.b", &[]);
    assert_eq!(table.claim(&existing), Ok(Claim::New));
    assert_eq!(
        table.claim(&incoming),
        Err(Box::new(ClaimRefusal::Collision(SymbolCollision {
            existing: existing.clone(),
            incoming: incoming.clone(),
            name: "miri.A.b".to_string(),
            namespace: Namespace::Link,
        })))
    );
    assert!(table.is_claimed(&existing));
    assert!(!table.is_claimed(&incoming));
}

#[test]
fn a_collision_names_both_definitions_as_the_source_writes_them() {
    let mut table = SymbolTable::default();
    table
        .claim(&Symbol::function(&ModuleId::Program, "pick$int", &[]))
        .expect("first claim is new");
    let refusal = table
        .claim(&Symbol::function(
            &ModuleId::Program,
            "pick",
            &[ty(TypeKind::Int)],
        ))
        .expect_err("a second definition of `miri.pick$int` collides");
    let ClaimRefusal::Collision(collision) = *refusal else {
        panic!("expected a collision, got {refusal:?}");
    };
    assert_eq!(
        collision.to_string(),
        "`pick$int` and `pick<int>` compile to the same symbol `miri.pick$int`"
    );
}

/// The refusal of a collision carries the collision's code, names both
/// definitions and asks for one of them to be renamed.
#[test]
fn a_collision_is_refused_with_the_symbol_collision_code() {
    let collision = SymbolCollision {
        existing: Symbol::method("A", &[], "b_c", &[]),
        incoming: Symbol::method("A_b", &[], "c", &[]),
        name: "A_b_c".to_string(),
        namespace: Namespace::Wgsl,
    };
    let properties = collision.refusal(Span::new(3, 9)).kind.properties();
    assert_eq!(properties.code, DiagnosticCode::MirSymbolCollision);
    assert_eq!(properties.code.to_string(), "MER_MIR_018");
    assert_eq!(
        properties.message.as_deref(),
        Some(
            "`A.b_c` and `A_b.c` are both reached from GPU code, \
             where both are declared as `A_b_c`"
        )
    );
    assert_eq!(
        properties.help.as_deref(),
        Some("rename one of the two definitions so that their compiled names differ")
    );
}

/// Two methods whose link names differ can still share a WGSL spelling; a
/// table keyed on that spelling refuses the second.
#[test]
fn a_wgsl_table_refuses_two_symbols_sharing_a_wgsl_name() {
    let mut table = SymbolTable::new(Namespace::Wgsl);
    let existing = Symbol::method("A", &[], "b_c", &[]);
    let incoming = Symbol::method("A_b", &[], "c", &[]);
    assert_ne!(existing.link_name(), incoming.link_name());
    assert_eq!(table.claim(&existing), Ok(Claim::New));
    assert_eq!(
        table.claim(&incoming),
        Err(Box::new(ClaimRefusal::Collision(SymbolCollision {
            existing,
            incoming,
            name: "A_b_c".to_string(),
            namespace: Namespace::Wgsl,
        })))
    );
}

/// `Option<Option<…<leaf>…>>` nested `depth` levels deep.
fn nested_options(depth: usize, leaf: TypeKind) -> Type {
    (0..depth).fold(ty(leaf), |inner, _| ty(TypeKind::Option(Box::new(inner))))
}

/// Every type nested past the depth types are named to spells alike, so a
/// symbol built from one stands for all of them: it is refused on every
/// claim, never found already lowered.
#[test]
fn a_symbol_with_an_unnameable_argument_is_never_claimed() {
    let strings = Symbol::function(
        &ModuleId::Program,
        "keep",
        &[nested_options(70, TypeKind::String)],
    );
    let ints = Symbol::function(
        &ModuleId::Program,
        "keep",
        &[nested_options(70, TypeKind::Int)],
    );
    assert_eq!(strings, ints);
    assert!(strings.has_an_unnameable_argument());
    let mut table = SymbolTable::default();
    for _ in 0..2 {
        assert_eq!(
            table.claim(&strings),
            Err(Box::new(ClaimRefusal::Unnameable(strings.clone())))
        );
    }
    assert!(!table.is_claimed(&strings));
}

/// The refusal of an unnameable symbol carries the instantiation code and
/// shows the argument it has no name for as `…`.
#[test]
fn an_unnameable_symbol_is_refused_with_the_instantiation_code() {
    let symbol = Symbol::method("W", &[nested_options(70, TypeKind::Int)], "get", &[]);
    let refusal = ClaimRefusal::Unnameable(symbol);
    let properties = refusal.refusal(Span::new(0, 0)).kind.properties();
    assert_eq!(
        properties.code,
        DiagnosticCode::MirInvalidInstantiationArgument
    );
    assert_eq!(
        properties.message.as_deref(),
        Some("`W<…>.get` has a type argument with no name the compiler can compile a body at")
    );
}

#[test]
fn a_nameable_symbol_has_no_unnameable_argument() {
    assert!(!Symbol::function(
        &ModuleId::Program,
        "keep",
        &[nested_options(3, TypeKind::Int)]
    )
    .has_an_unnameable_argument());
    assert!(!Symbol::entry().has_an_unnameable_argument());
}

#[test]
fn a_method_symbol_answers_which_method_of_its_owner_it_is() {
    let at_int = [ty(TypeKind::Int)];
    let symbol = Symbol::method("Box", &at_int, "get", &[]);
    assert_eq!(symbol.method_of("Box", &at_int), Some("get"));
    assert_eq!(symbol.method_of("Box", &[]), None);
    assert_eq!(symbol.method_of("Bo", &at_int), None);
    assert_eq!(
        Symbol::function(&ModuleId::Program, "Box_get", &at_int).method_of("Box", &at_int),
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

/// Every module a `use` can load by a path of one or two segments drawn
/// from `names`.
fn module_paths(names: &[String]) -> Vec<ModuleId> {
    let short: Vec<&String> = names.iter().filter(|name| name.len() <= 2).collect();
    let single = short.iter().map(|name| vec![(*name).clone()]);
    let double = short
        .iter()
        .filter(|name| name.len() == 1)
        .flat_map(|first| {
            short
                .iter()
                .filter(|name| name.len() == 1)
                .map(|second| vec![(*first).clone(), (*second).clone()])
        });
    single.chain(double).map(ModuleId::Imported).collect()
}

/// Every function `modules` can declare under `names`, at each of
/// `argument_lists`, with and without gpu residency — including a module
/// whose path ends in the name of a function or of a type with methods.
fn module_functions_named_from(
    modules: &[ModuleId],
    names: &[String],
    argument_lists: &[Vec<Type>],
) -> Vec<Symbol> {
    let functions = modules.iter().flat_map(|module| {
        names.iter().flat_map(move |name| {
            argument_lists
                .iter()
                .map(move |args| Symbol::function(module, name, args))
        })
    });
    let entries = modules
        .iter()
        .map(|module| Symbol::declared_function(module, "main"));
    with_and_without_residency(functions.chain(entries))
}

/// Every symbol a program can name from `names` and `argument_lists`.
fn symbols_named_from(names: &[String], argument_lists: &[Vec<Type>]) -> Vec<Symbol> {
    let mut symbols = vec![Symbol::entry()];
    for name in names {
        symbols.push(Symbol::runtime(name));
        for args in argument_lists {
            symbols.push(Symbol::function(&ModuleId::Program, name, args));
            symbols.push(Symbol::vtable(name, args));
            symbols.push(Symbol::type_thunk(ThunkKind::Drop, name, args));
            symbols.push(Symbol::type_thunk(ThunkKind::Decref, name, args));
            let target = Symbol::function(&ModuleId::Program, name, args).link_name();
            let closures = [
                Symbol::closure(ClosureKind::NestedFunction(name.clone()), 1, args),
                Symbol::closure(ClosureKind::FunctionReference(target), 1, &[]),
            ];
            symbols.extend(with_and_without_residency(
                std::iter::once(Symbol::function(&ModuleId::Program, name, args)).chain(closures),
            ));
        }
    }
    for id in [1, 11, 111] {
        for args in argument_lists {
            symbols.extend(with_and_without_residency([Symbol::closure(
                ClosureKind::Lambda,
                id,
                args,
            )]));
        }
    }
    let owners = names.iter().filter(|name| name.len() <= 3);
    for owner in owners {
        for method in names.iter().filter(|name| name.len() <= 3) {
            for args in argument_lists {
                symbols.extend(with_and_without_residency([
                    Symbol::method(owner, args, method, &[]),
                    Symbol::method(owner, &[], method, args),
                ]));
            }
        }
    }
    symbols
}

/// Each of `symbols`, then each again specialized for one and for two
/// gpu-resident buffers.
fn with_and_without_residency(symbols: impl IntoIterator<Item = Symbol>) -> Vec<Symbol> {
    let residencies: [&[(usize, DeviceHandleId)]; 2] = [
        &[(0, DeviceHandleId(1))],
        &[(0, DeviceHandleId(1)), (1, DeviceHandleId(11))],
    ];
    symbols
        .into_iter()
        .flat_map(|symbol| {
            let specialized = residencies.map(|residency| symbol.clone().with_residency(residency));
            std::iter::once(symbol).chain(specialized)
        })
        .collect()
}

/// A bounded regression sweep backing the injectivity argument in the
/// module documentation of `miri::mir::symbol`: every symbol built from short
/// identifiers over an alphabet of letters, digits and underscores — alone,
/// as methods and closures, as functions of the program or of a module, with
/// and without gpu residency — spells a link name no other one does. It cannot prove the grammar injective; it keeps a
/// change that breaks it from going unnoticed.
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
    let mut symbols = symbols_named_from(&names, &argument_lists);
    symbols.extend(module_functions_named_from(
        &module_paths(&names),
        &names,
        &argument_lists,
    ));
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
