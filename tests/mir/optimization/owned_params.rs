// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass deciding which parameters a function takes over from its
//! callers.
//!
//! Each program is a hand-built pair of bodies — a callee binding its managed
//! parameter to a local, and a caller passing it one — so each test pins the
//! exact shape that makes a parameter owned or keeps it borrowed.

use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{Type, TypeDeclarationKind, TypeKind, VEC3_TYPE_NAME};
use miri::ast::MemberVisibility;
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::body::DeviceHandleId;
use miri::mir::optimization::owned_params::decide_owned_parameters;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::{
    ClassDefinition, DropHookReach, GenericDefinition, MethodInfo, TypeDefinition,
};
use miri::mir::{
    ArgMode, Body, ExecutionModel, Local, LocalDecl, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::type_checker::context::{FieldInfo, StructDefinition};
use miri::type_checker::ModuleId;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::rc::Rc;

fn span() -> Span {
    Span::new(0, 0)
}

fn string_ty() -> Type {
    Type::new(TypeKind::String, span())
}

fn void_ty() -> Type {
    Type::new(TypeKind::Void, span())
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn stmt(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

fn bind(dest: usize, param: usize) -> Statement {
    stmt(StatementKind::Assign(
        place(dest),
        Rvalue::Use(Operand::Copy(place(param))),
    ))
}

fn storage_dead(local: usize) -> Statement {
    stmt(StatementKind::StorageDead(place(local)))
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    BasicBlockData {
        statements,
        terminator: Some(Terminator { kind, span: span() }),
        is_cleanup: false,
    }
}

fn body_of(local_tys: &[Type], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for local_ty in local_tys {
        body.new_local(LocalDecl::new(local_ty.clone(), span()));
    }
    body.basic_blocks = blocks;
    body
}

fn callee_symbol() -> Symbol {
    Symbol::declared_function(&ModuleId::Program, "callee")
}

fn call(func: Symbol, args: Vec<Operand>, destination: usize, target: usize) -> TerminatorKind {
    TerminatorKind::Call {
        func: Operand::function(func, span()),
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(destination),
        target: Some(BasicBlock(target)),
    }
}

/// `fn callee(param ty)`: `_2 = _1` in the entry block, which jumps to the
/// block ending both. Locals: 0 return, 1 the parameter, 2 its binding.
fn binding_callee_of(ty: Type) -> Body {
    body_of(
        &[void_ty(), ty.clone(), ty],
        1,
        vec![
            block(
                vec![bind(2, 1)],
                TerminatorKind::Goto {
                    target: BasicBlock(1),
                },
            ),
            block(
                vec![storage_dead(2), storage_dead(1)],
                TerminatorKind::Return,
            ),
        ],
    )
}

fn binding_callee() -> Body {
    binding_callee_of(string_ty())
}

/// A caller passing its local `_1` to `func`.
/// Locals: 0 return, 1 the argument, 2 the call's unmanaged result.
fn caller_of(func: Symbol) -> Body {
    body_of(
        &[void_ty(), string_ty(), void_ty()],
        0,
        vec![
            block(Vec::new(), call(func, vec![Operand::Copy(place(1))], 2, 1)),
            block(Vec::new(), TerminatorKind::Return),
        ],
    )
}

fn program(callee: Body) -> Vec<(Symbol, Body)> {
    vec![
        (callee_symbol(), callee),
        (Symbol::entry(), caller_of(callee_symbol())),
    ]
}

fn decide(program: &mut [(Symbol, Body)]) {
    decide_with(program, &definitions());
}

fn decide_with(program: &mut [(Symbol, Body)], definitions: &HashMap<String, TypeDefinition>) {
    decide_owned_parameters(program, &HashSet::new(), &DropHookReach::new(definitions));
}

fn named(name: &str, args: Vec<Type>) -> Type {
    let args = (!args.is_empty()).then(|| args.into_iter().map(type_expr_non_null).collect());
    Type::new(TypeKind::Custom(name.to_string(), args), span())
}

/// `class {name}` holding one field of type `field`, with a `fn drop(self)`
/// hook when `has_hook`, and generic over `T` when `is_generic`.
fn class_def(name: &str, field: Type, has_hook: bool, is_generic: bool) -> TypeDefinition {
    let drop_hook = MethodInfo {
        params: Vec::new(),
        is_out_flags: Vec::new(),
        return_type: void_ty(),
        visibility: MemberVisibility::Public,
        is_constructor: false,
        is_abstract: false,
        is_static: false,
        attributes: Vec::new(),
    };
    let generics = is_generic.then(|| {
        vec![GenericDefinition {
            name: "T".to_string(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }]
    });
    TypeDefinition::Class(ClassDefinition {
        name: name.to_string(),
        generics,
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![(
            "value".to_string(),
            FieldInfo {
                ty: field,
                mutable: true,
                visibility: MemberVisibility::Public,
                initializer: None,
            },
        )],
        methods: has_hook
            .then(|| BTreeMap::from([("drop".to_string(), drop_hook)]))
            .unwrap_or_default(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    })
}

/// A list class storing its elements out of its fields' sight, a class `R`
/// whose release runs a drop hook, a class `Plain` with none, and the vector
/// `Vec3`, all defined so that only the guard a test names keeps a parameter
/// borrowed.
fn definitions() -> HashMap<String, TypeDefinition> {
    let raw = Type::new(TypeKind::RawPtr, span());
    let int = Type::new(TypeKind::Int, span());
    HashMap::from([
        ("List".to_string(), class_def("List", raw, false, true)),
        ("R".to_string(), class_def("R", int.clone(), true, false)),
        ("Plain".to_string(), class_def("Plain", int, false, false)),
        (
            VEC3_TYPE_NAME.to_string(),
            TypeDefinition::Struct(StructDefinition {
                fields: Vec::new(),
                generics: None,
                module: String::new(),
            }),
        ),
    ])
}

fn list_of(element: Type) -> Type {
    named("List", vec![element])
}

fn assert_owned(program: &[(Symbol, Body)]) {
    assert_eq!(program[0].1.owned_params, vec![true]);
    assert_eq!(call_modes(&program[1].1), vec![ArgMode::Owned]);
}

/// The program `callee(param ty)` called once, its parameter bound once.
fn program_of(ty: Type) -> Vec<(Symbol, Body)> {
    let mut program = program(binding_callee_of(ty.clone()));
    program[1].1.local_decls[1].ty = ty;
    program
}

fn call_modes(body: &Body) -> Vec<ArgMode> {
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .find_map(|terminator| match &terminator.kind {
            TerminatorKind::Call { arg_modes, .. } => Some(arg_modes.clone()),
            _ => None,
        })
        .expect("the caller makes a call")
}

/// Assert the callee kept its parameter borrowed and no call passes it owned.
fn assert_borrowed(program: &[(Symbol, Body)]) {
    let (_, callee) = &program[0];
    assert!(
        !callee.owned_params.contains(&true),
        "the parameter was made owned: {:?}",
        callee.owned_params
    );
    assert!(
        !call_modes(&program[1].1).contains(&ArgMode::Owned),
        "a call passes an owned argument to a borrowing callee"
    );
}

#[test]
fn a_parameter_only_bound_to_a_local_is_owned_and_every_call_funds_it() {
    let mut program = program(binding_callee());
    decide(&mut program);

    let (_, callee) = &program[0];
    assert_eq!(callee.owned_params, vec![true]);
    assert_eq!(
        callee.basic_blocks[0].statements[0].kind,
        StatementKind::Assign(place(2), Rvalue::Use(Operand::Move(place(1)))),
        "the binding takes the parameter's reference over"
    );
    assert_eq!(call_modes(&program[1].1), vec![ArgMode::Owned]);
}

#[test]
fn a_parameter_read_a_second_time_stays_borrowed() {
    let mut callee = binding_callee();
    callee.basic_blocks[0].statements.insert(1, bind(2, 1));
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_binding_outside_the_entry_block_stays_borrowed() {
    let callee = body_of(
        &[void_ty(), string_ty(), string_ty()],
        1,
        vec![
            block(
                Vec::new(),
                TerminatorKind::Goto {
                    target: BasicBlock(1),
                },
            ),
            block(vec![bind(2, 1), storage_dead(2)], TerminatorKind::Return),
        ],
    );
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_binding_released_in_the_entry_block_stays_borrowed() {
    let callee = body_of(
        &[void_ty(), string_ty(), string_ty()],
        1,
        vec![block(
            vec![bind(2, 1), storage_dead(2), storage_dead(1)],
            TerminatorKind::Return,
        )],
    );
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn an_out_parameter_stays_borrowed() {
    let mut callee = binding_callee();
    callee.out_params = vec![true];
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_parameter_typed_by_a_type_parameter_stays_borrowed() {
    let element = Type::new(TypeKind::Custom("T".to_string(), None), span());
    let optional = Type::new(TypeKind::Option(Box::new(element)), span());
    let mut callee = binding_callee_of(optional);
    callee.type_params.insert("T".to_string());
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_callee_running_on_a_gpu_stays_borrowed() {
    let mut callee = binding_callee();
    callee.execution_model = ExecutionModel::GpuDevice;
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_callee_a_gpu_body_calls_stays_borrowed() {
    let mut program = program(binding_callee());
    let mut gpu_caller = caller_of(callee_symbol());
    gpu_caller.execution_model = ExecutionModel::GpuDevice;
    program.push((
        Symbol::declared_function(&ModuleId::Program, "gpu_caller"),
        gpu_caller,
    ));
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn the_entry_point_keeps_its_parameters_borrowed() {
    let mut program = vec![
        (Symbol::entry(), binding_callee()),
        (
            Symbol::declared_function(&ModuleId::Program, "caller"),
            caller_of(Symbol::entry()),
        ),
    ];
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_statically_called_method_binding_its_parameter_owns_it() {
    let symbol = Symbol::method("Owner", &[], "callee", &[]);
    let mut program = vec![
        (symbol.clone(), binding_callee()),
        (
            Symbol::declared_function(&ModuleId::Program, "caller"),
            caller_of(symbol),
        ),
    ];
    decide(&mut program);
    assert_owned(&program);
}

#[test]
fn a_receiver_bound_to_a_local_stays_borrowed() {
    let mut callee = binding_callee();
    callee.receiver = Some(Local(1));
    let symbol = Symbol::method("Owner", &[], "callee", &[]);
    let mut program = vec![
        (symbol.clone(), callee),
        (
            Symbol::declared_function(&ModuleId::Program, "caller"),
            caller_of(symbol),
        ),
    ];
    decide(&mut program);
    assert_borrowed(&program);
}

/// Codegen calls an element method through a runtime thunk at every
/// instantiation of its class, lending what it passes.
#[test]
fn a_method_a_runtime_thunk_calls_stays_borrowed() {
    let int = Type::new(TypeKind::Int, span());
    let symbol = Symbol::method("Owner", &[int], "equals", &[]);
    let mut program = vec![
        (symbol.clone(), binding_callee()),
        (
            Symbol::declared_function(&ModuleId::Program, "caller"),
            caller_of(symbol),
        ),
    ];
    decide(&mut program);
    assert_borrowed(&program);
}

fn holder_symbol() -> Symbol {
    Symbol::method("Holder", &[], "init", &[])
}

/// `class Holder` with one `String` field, beside the shared definitions.
fn holder_definitions() -> HashMap<String, TypeDefinition> {
    let mut definitions = definitions();
    definitions.insert(
        "Holder".to_string(),
        class_def("Holder", string_ty(), false, false),
    );
    definitions
}

/// `_1.0`, the field of the receiver.
fn receiver_field() -> Place {
    Place {
        local: Local(1),
        projection: vec![PlaceElem::Field(0)],
    }
}

fn store_parameter() -> Statement {
    stmt(StatementKind::Reassign(
        receiver_field(),
        Rvalue::Use(Operand::Copy(place(2))),
    ))
}

/// `fn init(self, value String)` storing `value` into the receiver's field,
/// then discarding the store's value as an expression statement does.
/// Locals: 0 return, 1 `self`, 2 `value`, 3 the discarded value.
fn storing_method(statements: Vec<Statement>) -> Body {
    let holder = named("Holder", Vec::new());
    let mut body = body_of(
        &[void_ty(), holder, string_ty(), string_ty()],
        2,
        vec![block(statements, TerminatorKind::Return)],
    );
    body.receiver = Some(Local(1));
    body.rc_classifier = RcClassifier::new(Rc::new(holder_definitions()));
    body
}

fn storing_statements() -> Vec<Statement> {
    vec![
        store_parameter(),
        bind(3, 2),
        storage_dead(3),
        storage_dead(2),
        storage_dead(1),
    ]
}

/// A caller handing `Holder.init` its receiver `_1` and a string `_3`.
/// Locals: 0 return, 1 the receiver, 2 the unmanaged result, 3 the string.
fn holder_caller() -> Body {
    let holder = named("Holder", Vec::new());
    body_of(
        &[void_ty(), holder, void_ty(), string_ty()],
        0,
        vec![
            block(
                Vec::new(),
                call(
                    holder_symbol(),
                    vec![Operand::Copy(place(1)), Operand::Copy(place(3))],
                    2,
                    1,
                ),
            ),
            block(Vec::new(), TerminatorKind::Return),
        ],
    )
}

fn holder_program(statements: Vec<Statement>) -> Vec<(Symbol, Body)> {
    vec![
        (holder_symbol(), storing_method(statements)),
        (Symbol::entry(), holder_caller()),
    ]
}

#[test]
fn a_parameter_only_stored_into_a_field_is_owned_and_moved_there() {
    let mut program = holder_program(storing_statements());
    decide_with(&mut program, &holder_definitions());

    let (_, init) = &program[0];
    assert_eq!(init.owned_params, vec![false, true]);
    assert_eq!(
        init.basic_blocks[0].statements[0].kind,
        StatementKind::Reassign(receiver_field(), Rvalue::Use(Operand::Move(place(2)))),
        "the store takes the parameter's reference over"
    );
    assert!(
        !init.basic_blocks[0]
            .statements
            .iter()
            .any(|statement| matches!(
                &statement.kind,
                StatementKind::Assign(_, Rvalue::Use(operand)) if operand.place() == Some(&place(2))
            )),
        "the discarded value no longer reads the moved parameter: {init}"
    );
    assert_eq!(
        call_modes(&program[1].1),
        vec![ArgMode::Borrowed, ArgMode::Owned]
    );
}

#[test]
fn a_parameter_stored_twice_stays_borrowed() {
    let mut statements = storing_statements();
    statements.insert(1, store_parameter());
    let mut program = holder_program(statements);
    decide_with(&mut program, &holder_definitions());
    assert!(!program[0].1.owned_params.contains(&true));
    assert!(!call_modes(&program[1].1).contains(&ArgMode::Owned));
}

#[test]
fn a_stored_value_read_again_stays_borrowed() {
    let mut statements = storing_statements();
    statements.insert(2, bind(0, 3));
    let mut program = holder_program(statements);
    decide_with(&mut program, &holder_definitions());
    assert!(!program[0].1.owned_params.contains(&true));
}

/// A store whose field type the body cannot read may hold the value as its
/// bytes, taking no reference.
#[test]
fn a_store_into_a_field_of_unknown_type_stays_borrowed() {
    let mut program = holder_program(storing_statements());
    program[0].1.rc_classifier = RcClassifier::default();
    decide_with(&mut program, &holder_definitions());
    assert!(!program[0].1.owned_params.contains(&true));
}

#[test]
fn a_method_codegen_reaches_through_a_vtable_stays_borrowed() {
    let mut program = holder_program(storing_statements());
    let reached: HashSet<Symbol> = [holder_symbol()].into_iter().collect();
    decide_owned_parameters(
        &mut program,
        &reached,
        &DropHookReach::new(&holder_definitions()),
    );
    assert!(!program[0].1.owned_params.contains(&true));
    assert!(!call_modes(&program[1].1).contains(&ArgMode::Owned));
}

#[test]
fn a_callee_codegen_calls_outside_the_program_stays_borrowed() {
    let mut program = program(binding_callee());
    let synthesized: HashSet<Symbol> = [callee_symbol()].into_iter().collect();
    decide_owned_parameters(
        &mut program,
        &synthesized,
        &DropHookReach::new(&definitions()),
    );
    assert_borrowed(&program);
}

#[test]
fn a_callee_named_as_a_value_stays_borrowed() {
    let mut program = program(binding_callee());
    let (_, caller) = &mut program[1];
    caller.new_local(LocalDecl::new(
        Type::new(TypeKind::Identifier, span()),
        span(),
    ));
    caller.basic_blocks[0]
        .statements
        .push(stmt(StatementKind::Assign(
            place(3),
            Rvalue::Use(Operand::function(callee_symbol(), span())),
        )));
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_list_of_plain_values_is_owned() {
    let mut program = program_of(list_of(named("Plain", Vec::new())));
    decide(&mut program);
    assert_owned(&program);
}

#[test]
fn a_parameter_whose_release_runs_a_drop_hook_stays_borrowed() {
    let mut program = program_of(named("R", Vec::new()));
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_list_whose_elements_run_a_drop_hook_stays_borrowed() {
    let mut program = program_of(list_of(named("R", Vec::new())));
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_vector_parameter_stays_borrowed() {
    let float = Type::new(TypeKind::F32, span());
    let mut program = program_of(named(VEC3_TYPE_NAME, vec![float]));
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_closure_body_with_captures_stays_borrowed() {
    let mut callee = binding_callee();
    callee.env_capture_locals = vec![Local(2)];
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_symbol_lowered_into_two_bodies_stays_borrowed() {
    let mut program = program(binding_callee());
    program.push((callee_symbol(), binding_callee()));
    decide(&mut program);
    assert!(
        program
            .iter()
            .all(|(_, body)| !body.owned_params.contains(&true)),
        "a body under a symbol lowered twice took its parameter over"
    );
    assert!(!call_modes(&program[1].1).contains(&ArgMode::Owned));
}

#[test]
fn a_callee_called_at_another_arity_stays_borrowed() {
    let mut program = program(binding_callee());
    let TerminatorKind::Call { args, .. } = &mut program[1].1.basic_blocks[0]
        .terminator
        .as_mut()
        .expect("the caller ends in a call")
        .kind
    else {
        panic!("the caller ends in a call");
    };
    args.push(Operand::Copy(place(1)));
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_parameter_holding_a_device_buffer_stays_borrowed() {
    let mut callee = binding_callee();
    callee.local_decls[1].device_handle = Some(DeviceHandleId(1));
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_binding_holding_a_device_buffer_stays_borrowed() {
    let mut callee = binding_callee();
    callee.local_decls[2].device_handle = Some(DeviceHandleId(1));
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_binding_in_an_entry_block_a_loop_returns_to_stays_borrowed() {
    let mut callee = binding_callee();
    callee.basic_blocks[1].terminator = Some(Terminator {
        kind: TerminatorKind::Goto {
            target: BasicBlock(0),
        },
        span: span(),
    });
    let mut program = program(callee);
    decide(&mut program);
    assert_borrowed(&program);
}

#[test]
fn a_function_nothing_calls_keeps_its_parameters_borrowed() {
    let mut program = vec![(callee_symbol(), binding_callee())];
    decide(&mut program);
    assert!(!program[0].1.owned_params.contains(&true));
    assert_eq!(
        program[0].1.basic_blocks[0].statements[0].kind,
        bind(2, 1).kind,
        "the binding still copies the parameter"
    );
}

/// `fn callee(a String, b String)` binding only `b`: locals 0 return, 1 `a`,
/// 2 `b`, 3 `b`'s binding.
fn second_binding_callee() -> Body {
    body_of(
        &[void_ty(), string_ty(), string_ty(), string_ty()],
        2,
        vec![
            block(
                vec![bind(3, 2)],
                TerminatorKind::Goto {
                    target: BasicBlock(1),
                },
            ),
            block(vec![storage_dead(3)], TerminatorKind::Return),
        ],
    )
}

#[test]
fn only_the_bound_parameter_of_two_is_owned() {
    let mut caller = caller_of(callee_symbol());
    let TerminatorKind::Call { args, .. } = &mut caller.basic_blocks[0]
        .terminator
        .as_mut()
        .expect("the caller ends in a call")
        .kind
    else {
        panic!("the caller ends in a call");
    };
    args.push(Operand::Copy(place(1)));
    let mut program = vec![
        (callee_symbol(), second_binding_callee()),
        (Symbol::entry(), caller),
    ];
    decide(&mut program);
    assert_eq!(program[0].1.owned_params, vec![false, true]);
    assert_eq!(
        call_modes(&program[1].1),
        vec![ArgMode::Borrowed, ArgMode::Owned]
    );
}
