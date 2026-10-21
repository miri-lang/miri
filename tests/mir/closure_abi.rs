// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The closure ABI pass: each closure a runtime call takes is followed by the
//! address of the trampoline the runtime calls it through, at the argument
//! position the runtime parameter table gives it once every earlier element
//! and closure has gained its second word.

use miri::ast::common::Parameter;
use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{FunctionTypeData, Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::closure_abi::pass_closures_through;
use miri::mir::symbol::{TrampolineArgument, TrampolineShape, TrampolineWord};
use miri::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, ExecutionModel, Local, LocalDecl, Operand, Place,
    Terminator, TerminatorKind,
};
use miri::runtime_fns::{ClosureContract, RuntimeParamAbi, RuntimeParamTable};

/// A runtime surface of one entry point, `sort`, with the parameters `abis`.
struct Table {
    abis: Vec<RuntimeParamAbi>,
    closures: Vec<usize>,
}

impl Table {
    fn of(abis: &[RuntimeParamAbi]) -> Self {
        let closures = abis
            .iter()
            .enumerate()
            .filter(|(_, abi)| matches!(abi, RuntimeParamAbi::Closure(_)))
            .map(|(position, _)| position)
            .collect();
        Self {
            abis: abis.to_vec(),
            closures,
        }
    }
}

impl RuntimeParamTable for Table {
    fn param_abi(&self, name: &str, position: usize) -> RuntimeParamAbi {
        match (name, self.abis.get(position)) {
            ("sort", Some(abi)) => *abi,
            _ => RuntimeParamAbi::Word,
        }
    }

    fn closure_positions(&self, name: &str) -> &[usize] {
        if name == "sort" {
            &self.closures
        } else {
            &[]
        }
    }
}

/// A closure over two elements of the list it sorts.
const COMPARATOR: RuntimeParamAbi = RuntimeParamAbi::Closure(ClosureContract {
    arity: 2,
    elements_of: Some(0),
});

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn function_of(params: usize) -> TypeKind {
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
        return_type: Some(Box::new(type_expr_non_null(ty(TypeKind::Int)))),
    }))
}

/// A body whose one block calls `sort` with a local of each of `args`.
fn calling_sort_with(args: &[TypeKind]) -> Body {
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
            func: Operand::runtime("sort", Span::default()),
            arg_modes: vec![ArgMode::Borrowed; operands.len()],
            args: operands,
            arg_handles: Vec::new(),
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
    TypeKind::List(Box::new(type_expr_non_null(ty(TypeKind::Int))))
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
    let table = Table::of(&[RuntimeParamAbi::Word, RuntimeParamAbi::Element, COMPARATOR]);
    let mut body = calling_sort_with(&[
        list_of_int(),
        TypeKind::RawPtr,
        TypeKind::Int,
        function_of(2),
    ]);
    pass_closures_through(&mut body, &table).expect("the call keeps the contract");
    assert_eq!(
        trampolines(&body),
        vec![None, None, None, None, word_comparator()]
    );
}

#[test]
fn a_closure_that_is_not_the_last_parameter_keeps_the_ones_after_it_in_order() {
    let table = Table::of(&[RuntimeParamAbi::Word, COMPARATOR, RuntimeParamAbi::Word]);
    let mut body = calling_sort_with(&[list_of_int(), function_of(2), TypeKind::Int]);
    pass_closures_through(&mut body, &table).expect("the call keeps the contract");
    assert_eq!(
        trampolines(&body),
        vec![None, None, word_comparator(), None]
    );
}

#[test]
fn each_of_two_closures_is_followed_by_its_own_trampoline() {
    let table = Table::of(&[RuntimeParamAbi::Word, COMPARATOR, COMPARATOR]);
    let mut body = calling_sort_with(&[list_of_int(), function_of(2), function_of(2)]);
    pass_closures_through(&mut body, &table).expect("the call keeps the contract");
    assert_eq!(
        trampolines(&body),
        vec![None, None, word_comparator(), None, word_comparator()]
    );
}

#[test]
fn a_closure_of_another_arity_than_the_contract_is_refused() {
    let table = Table::of(&[RuntimeParamAbi::Word, COMPARATOR]);
    let mut body = calling_sort_with(&[list_of_int(), function_of(3)]);
    let refusal = pass_closures_through(&mut body, &table).expect_err("the arity is refused");
    assert!(
        refusal.contains("taking 3 arguments, where the runtime passes 2"),
        "{refusal}"
    );
}

#[test]
fn a_closure_over_values_wider_than_the_list_s_elements_is_refused() {
    let table = Table::of(&[RuntimeParamAbi::Word, COMPARATOR]);
    let narrow = TypeKind::List(Box::new(type_expr_non_null(ty(TypeKind::I32))));
    let mut body = calling_sort_with(&[narrow, function_of(2)]);
    let refusal = pass_closures_through(&mut body, &table).expect_err("the width is refused");
    assert!(
        refusal.contains("not read as the collection holds"),
        "{refusal}"
    );
}

#[test]
fn a_parameter_the_table_calls_a_closure_but_the_call_does_not_hand_one_is_refused() {
    let table = Table::of(&[RuntimeParamAbi::Word, COMPARATOR]);
    let mut body = calling_sort_with(&[list_of_int(), TypeKind::Int]);
    let refusal = pass_closures_through(&mut body, &table).expect_err("a word is no closure");
    assert!(refusal.contains("is not handed a closure"), "{refusal}");
}

#[test]
fn a_call_to_an_entry_point_with_no_closure_parameter_is_left_alone() {
    let table = Table::of(&[RuntimeParamAbi::Word, COMPARATOR]);
    let mut body = calling_sort_with(&[list_of_int(), function_of(2)]);
    let Some(TerminatorKind::Call { func, .. }) = body.basic_blocks[0]
        .terminator
        .as_mut()
        .map(|t| &mut t.kind)
    else {
        panic!("the body ends without its call");
    };
    *func = Operand::runtime("reverse", Span::default());
    pass_closures_through(&mut body, &table).expect("nothing to check");
    assert_eq!(trampolines(&body), vec![None, None]);
}
