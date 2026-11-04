// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The closure ABI pass: each closure a runtime call takes is followed by the
//! address of the trampoline the runtime calls it through, at the argument
//! position the runtime parameter table gives it once every earlier element
//! and closure has gained its second word.

use crate::mir::utils::runtime_callee_stating;
use miri::ast::common::Parameter;
use miri::ast::factory::type_expr_non_null;
use miri::ast::type_arguments::type_argument;
use miri::ast::types::{FunctionTypeData, Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::closure_abi::pass_closures_with_trampolines;
use miri::mir::symbol::{TrampolineArgument, TrampolineShape, TrampolineWord};
use miri::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place,
    Terminator, TerminatorKind,
};
use miri::runtime_effects::{ClosureContract, ElementRole, ParamUse};

/// A closure over two elements of the list it sorts.
const COMPARATOR: ParamUse = ParamUse::Closure(ClosureContract {
    arity: 2,
    elements_of: Some(0),
});

/// The list `sort` is handed, travelling as one word.
const LIST: ParamUse = ParamUse::Reads;

/// A word `sort` is handed.
const WORD: ParamUse = ParamUse::Scalar;

/// An element `sort` is handed, travelling as its address and byte count.
const ELEMENT: ParamUse = ParamUse::LooksUp(ElementRole::Element);

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn function_of(params: usize) -> TypeKind {
    function_returning(params, TypeKind::Int)
}

fn function_returning(params: usize, result: TypeKind) -> TypeKind {
    let param = || Parameter {
        name: String::new(),
        name_span: Span::default(),
        typ: Box::new(type_expr_non_null(ty(TypeKind::Int))),
        guard: None,
        default_value: None,
        is_out: false,
        residency: None,
    };
    TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: (0..params).map(|_| param()).collect(),
        return_type: Some(Box::new(type_expr_non_null(ty(result)))),
    }))
}

/// A body whose one block calls `sort`, declared with `params`, with a local
/// of each of `args`.
fn calling_sort_with(params: &[ParamUse], args: &[TypeKind]) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.local_decls
        .push(LocalDecl::new(ty(TypeKind::Void), Span::default()));
    let operands = args
        .iter()
        .map(|kind| {
            let local = body.new_local(LocalDecl::new(ty(kind.clone()), Span::default()));
            Operand::Copy(Place::new(local))
        })
        .collect::<Vec<_>>();
    let terminator = Terminator::new(
        TerminatorKind::Call {
            func: Operand::runtime(
                runtime_callee_stating("sort", params.to_vec()),
                Span::default(),
            ),
            arg_modes: vec![ArgMode::Borrowed; operands.len()],
            args: operands,
            arg_handles: Vec::new(),
            flags: miri::mir::CallFlags::default(),
            destination: Place::new(Local(0)),
            target: Some(BasicBlock(0)),
        },
        Span::default(),
    );
    body.basic_blocks
        .push(BasicBlockData::new(Some(terminator)));
    body
}

fn list_of_int() -> TypeKind {
    TypeKind::List(Box::new(type_argument(ty(TypeKind::Int))))
}

/// For each argument of the call `body` ends with, the trampoline it names,
/// or `None` for an argument that is no trampoline.
fn trampolines(body: &Body) -> Vec<Option<TrampolineShape>> {
    let Some(TerminatorKind::Call {
        args, arg_modes, ..
    }) = body.basic_blocks[0].terminator.as_ref().map(|t| &t.kind)
    else {
        panic!("the body ends without its call");
    };
    assert_eq!(args.len(), arg_modes.len(), "every argument keeps a mode");
    args.iter()
        .map(|arg| match arg {
            Operand::Function(function) => function.symbol.trampoline_shape().cloned(),
            Operand::Copy(_) | Operand::Move(_) | Operand::Constant(_) => None,
        })
        .collect()
}

fn word_comparator() -> Option<TrampolineShape> {
    Some(TrampolineShape {
        arguments: vec![TrampolineArgument::Word(TrampolineWord::Pointer); 2],
        result: Some(TrampolineWord::Pointer),
    })
}

#[test]
fn a_trampoline_follows_a_closure_after_an_element_parameter_s_two_words() {
    let mut body = calling_sort_with(
        &[LIST, ELEMENT, COMPARATOR],
        &[
            list_of_int(),
            TypeKind::RawPtr,
            TypeKind::Int,
            function_of(2),
        ],
    );
    pass_closures_with_trampolines(&mut body).expect("the call keeps the contract");
    assert_eq!(
        trampolines(&body),
        vec![None, None, None, None, word_comparator()]
    );
}

#[test]
fn a_closure_that_is_not_the_last_parameter_keeps_the_ones_after_it_in_order() {
    let mut body = calling_sort_with(
        &[LIST, COMPARATOR, WORD],
        &[list_of_int(), function_of(2), TypeKind::Int],
    );
    pass_closures_with_trampolines(&mut body).expect("the call keeps the contract");
    assert_eq!(
        trampolines(&body),
        vec![None, None, word_comparator(), None]
    );
}

#[test]
fn each_of_two_closures_is_followed_by_its_own_trampoline() {
    let mut body = calling_sort_with(
        &[LIST, COMPARATOR, COMPARATOR],
        &[list_of_int(), function_of(2), function_of(2)],
    );
    pass_closures_with_trampolines(&mut body).expect("the call keeps the contract");
    assert_eq!(
        trampolines(&body),
        vec![None, None, word_comparator(), None, word_comparator()]
    );
}

#[test]
fn a_closure_of_another_arity_than_the_contract_is_refused() {
    let mut body = calling_sort_with(&[LIST, COMPARATOR], &[list_of_int(), function_of(3)]);
    let refusal = pass_closures_with_trampolines(&mut body).expect_err("the arity is refused");
    assert!(
        refusal.contains("taking 3 arguments, where the runtime passes 2"),
        "{refusal}"
    );
}

#[test]
fn a_closure_over_values_wider_than_the_list_s_elements_is_refused() {
    let narrow = TypeKind::List(Box::new(type_argument(ty(TypeKind::I32))));
    let mut body = calling_sort_with(&[LIST, COMPARATOR], &[narrow, function_of(2)]);
    let refusal = pass_closures_with_trampolines(&mut body).expect_err("the width is refused");
    assert!(
        refusal.contains("not read as the collection holds"),
        "{refusal}"
    );
}

/// A value held as its bytes comes back from a Miri call in registers or
/// through the caller's slot, and a trampoline stores only a word: a closure
/// handed to the runtime that answers one is refused.
#[test]
fn a_closure_answering_a_value_held_as_its_bytes_is_refused() {
    let answer = TypeKind::Option(Box::new(ty(TypeKind::Int)));
    let mut body = calling_sort_with(
        &[LIST, COMPARATOR],
        &[list_of_int(), function_returning(2, answer)],
    );
    let refusal = pass_closures_with_trampolines(&mut body).expect_err("a held result is refused");
    assert!(
        refusal.contains("which a trampoline cannot store"),
        "{refusal}"
    );
}

#[test]
fn a_parameter_the_table_calls_a_closure_but_the_call_does_not_hand_one_is_refused() {
    let mut body = calling_sort_with(&[LIST, COMPARATOR], &[list_of_int(), TypeKind::Int]);
    let refusal = pass_closures_with_trampolines(&mut body).expect_err("a word is no closure");
    assert!(refusal.contains("is not handed a closure"), "{refusal}");
}

#[test]
fn a_call_to_an_entry_point_with_no_closure_parameter_is_left_alone() {
    let mut body = calling_sort_with(&[LIST, COMPARATOR], &[list_of_int(), function_of(2)]);
    let Some(TerminatorKind::Call { func, .. }) = body.basic_blocks[0]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    else {
        panic!("the body ends without its call");
    };
    *func = Operand::runtime(
        runtime_callee_stating("reverse", Vec::new()),
        Span::default(),
    );
    pass_closures_with_trampolines(&mut body).expect("nothing to check");
    assert_eq!(trampolines(&body), vec![None, None]);
}
