// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The inliner's policy, and an inline run through Cranelift's own inliner
//! end to end, so a change in how Cranelift copies a callee fails here.

use super::super::source_points::{SourcePoint, SourcePoints};
use super::super::{build_isa, CraneliftOptions, OptLevel, TranslatedFunction};
use super::*;
use crate::error::SourceId;
use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::ir::{
    types, AbiParam, ExtFuncData, ExternalName, InstBuilder, MemFlags, Signature, StackSlotData,
    StackSlotKind, UserExternalName, UserFuncName,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::FuncId;

#[test]
fn a_callee_called_once_may_be_larger_than_one_called_from_several_places() {
    assert_eq!(size_limit(0), SMALL_CALLEE_SIZE);
    assert_eq!(size_limit(1), SINGLE_CALL_SITE_CALLEE_SIZE);
    assert_eq!(size_limit(2), SMALL_CALLEE_SIZE);
    assert!(SMALL_CALLEE_SIZE < SINGLE_CALL_SITE_CALLEE_SIZE);
}

#[test]
fn a_small_caller_grows_to_four_times_its_size() {
    let mut budget = GrowthBudget::new(10, false);
    assert_eq!(budget.instructions, 30);
    assert!(budget.spend(20, 0));
    assert!(!budget.spend(11, 0));
    assert!(budget.spend(10, 0));
    assert_eq!(budget.instructions, 0);
}

#[test]
fn a_large_caller_grows_to_two_thousand_instructions_at_most() {
    assert_eq!(GrowthBudget::new(600, false).instructions, 1400);
    assert_eq!(GrowthBudget::new(2000, false).instructions, 0);
    assert_eq!(GrowthBudget::new(usize::MAX, false).instructions, 0);
}

#[test]
fn the_added_stack_is_capped_lower_for_a_recursive_caller() {
    let mut budget = GrowthBudget::new(100, false);
    assert!(budget.spend(1, 500));
    assert!(!budget.spend(1, 13));
    assert!(budget.spend(1, 12));
    let mut recursive = GrowthBudget::new(100, true);
    assert!(!recursive.spend(1, 129));
    assert!(recursive.spend(1, 128));
    assert!(!GrowthBudget::new(100, false).spend(1, u64::MAX));
}

fn signature(param: types::Type) -> Signature {
    let mut signature = Signature::new(CallConv::triple_default(&target_lexicon::Triple::host()));
    signature.params.push(AbiParam::new(param));
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

/// `fn(i64) i64` returning its argument plus one, its two instructions on
/// points 0 and 1 of its table.
fn callee() -> (Function, SourcePoints) {
    let mut points = SourcePoints::default();
    let at = |points: &mut SourcePoints, offset| {
        let point = SourcePoint {
            source: SourceId::new(1),
            offset,
            inlined_at: None,
        };
        points.location_of(point).unwrap_or_default()
    };
    let mut function =
        Function::with_name_signature(UserFuncName::default(), signature(types::I64));
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let value = builder.block_params(entry)[0];
    builder.set_srcloc(at(&mut points, 10));
    let sum = builder.ins().iadd_imm(value, 1);
    builder.set_srcloc(at(&mut points, 20));
    builder.ins().return_(&[sum]);
    builder.seal_all_blocks();
    builder.finalize();
    (function, points)
}

/// A caller calling function 1 through a call declared `fn(param) i64`, the
/// call on point 0 of its table.
fn caller(param: types::Type) -> (Function, SourcePoints) {
    let mut points = SourcePoints::default();
    let call_point = SourcePoint {
        source: SourceId::PROGRAM,
        offset: 5,
        inlined_at: None,
    };
    let call_site = points.location_of(call_point).unwrap_or_default();
    let mut function =
        Function::with_name_signature(UserFuncName::default(), signature(types::I64));
    let name = function.declare_imported_user_function(UserExternalName::new(0, 1));
    let declared = function.import_signature(signature(param));
    let callee = function.import_function(ExtFuncData {
        name: ExternalName::User(name),
        signature: declared,
        colocated: false,
    });
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let mut argument = builder.block_params(entry)[0];
    if param != types::I64 {
        argument = builder.ins().ireduce(param, argument);
    }
    builder.set_srcloc(call_site);
    let call = builder.ins().call(callee, &[argument]);
    let result = builder.inst_results(call)[0];
    builder.ins().return_(&[result]);
    builder.seal_all_blocks();
    builder.finalize();
    (function, points)
}

fn translated(
    name: &'static str,
    id: u32,
    (function, source_points): (Function, SourcePoints),
) -> TranslatedFunction<'static> {
    TranslatedFunction {
        name,
        id: FuncId::from_u32(id),
        function,
        source_points,
        is_entry: false,
    }
}

/// Run the inliner over the caller and the callee, returning the caller as
/// it is handed to be defined.
fn inline_caller(param: types::Type) -> TranslatedFunction<'static> {
    let options = CraneliftOptions {
        opt_level: OptLevel::Speed,
        ..CraneliftOptions::default()
    };
    let isa = build_isa(target_lexicon::Triple::host(), &options)
        .unwrap_or_else(|e| panic!("the host target builds: {e}"));
    let functions = vec![
        translated("miri.caller", 0, caller(param)),
        translated("miri.callee", 1, callee()),
    ];
    let mut defined = Vec::new();
    inline_and_define(functions, &*isa, |position, function| {
        defined.push((position, function));
        Ok(())
    })
    .unwrap_or_else(|e| panic!("inlining succeeds: {e}"));
    let order: Vec<usize> = defined.iter().map(|(position, _)| *position).collect();
    assert_eq!(order, vec![1, 0], "the callee is defined before its caller");
    defined
        .into_iter()
        .find_map(|(position, function)| (position == 0).then_some(function))
        .unwrap_or_else(|| panic!("the caller was defined"))
}

fn has_call(function: &Function) -> bool {
    (function.layout.blocks()).any(|block| {
        (function.layout.block_insts(block))
            .any(|inst| function.dfg.insts[inst].opcode() == Opcode::Call)
    })
}

#[test]
fn a_typical_inline_lines_up_and_keeps_the_callees_points() {
    let caller = inline_caller(types::I64);
    assert!(!has_call(&caller.function), "the call was not inlined");
    let points: Vec<Option<SourcePoint>> = (caller.function.layout.blocks())
        .flat_map(|block| caller.function.layout.block_insts(block))
        .map(|inst| caller.source_points.point_at(caller.function.srcloc(inst)))
        .collect();
    for offset in [10, 20] {
        let inlined = SourcePoint {
            source: SourceId::new(1),
            offset,
            inlined_at: Some(0),
        };
        assert!(
            points.contains(&Some(inlined)),
            "no copy carries the callee's point at {offset}, inlined at the call: \
             Cranelift's copy order changed: {points:?}"
        );
    }
}

#[test]
fn a_call_whose_signature_is_not_the_callees_keeps_its_call() {
    let caller = inline_caller(types::I32);
    assert!(has_call(&caller.function), "a mismatched call was inlined");
}

/// The point `offset` of a callee's table, in source 1.
fn callee_point(points: &mut SourcePoints, offset: u32) -> cranelift_codegen::ir::SourceLoc {
    let point = SourcePoint {
        source: SourceId::new(1),
        offset,
        inlined_at: None,
    };
    points.location_of(point).unwrap_or_default()
}

/// `fn(i64) i64` returning one more than the `i64` its argument points at,
/// its load on point 10 and its addition on point 20.
fn reading_callee() -> (Function, SourcePoints) {
    let mut points = SourcePoints::default();
    let mut function =
        Function::with_name_signature(UserFuncName::default(), signature(types::I64));
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let address = builder.block_params(entry)[0];
    builder.set_srcloc(callee_point(&mut points, 10));
    let value = builder.ins().load(types::I64, MemFlags::new(), address, 0);
    builder.set_srcloc(callee_point(&mut points, 20));
    let sum = builder.ins().iadd_imm(value, 1);
    builder.ins().return_(&[sum]);
    builder.seal_all_blocks();
    builder.finalize();
    (function, points)
}

/// `fn(i64) i64` holding its argument in a slot of `size` bytes and
/// returning one more than what it reads back.
fn slot_holding_callee(size: u32) -> (Function, SourcePoints) {
    let mut function =
        Function::with_name_signature(UserFuncName::default(), signature(types::I64));
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let slot =
        builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, size, 3));
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    let argument = builder.block_params(entry)[0];
    builder.ins().stack_store(argument, slot, 0);
    let value = builder.ins().stack_load(types::I64, slot, 0);
    let sum = builder.ins().iadd_imm(value, 1);
    builder.ins().return_(&[sum]);
    builder.seal_all_blocks();
    builder.finalize();
    (function, SourcePoints::default())
}

/// A caller storing its argument in a slot of its own and calling
/// function 1 with the slot's address, the call on point 0 of its table.
fn slot_passing_caller() -> (Function, SourcePoints) {
    let (mut function, points) = caller(types::I64);
    let slot =
        function.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, 8, 3));
    let call = (function.layout.blocks())
        .flat_map(|block| function.layout.block_insts(block))
        .find(|&inst| function.dfg.insts[inst].opcode() == Opcode::Call)
        .unwrap_or_else(|| panic!("the caller calls"));
    let argument = function.dfg.inst_args(call)[0];
    let mut cursor = FuncCursor::new(&mut function).at_inst(call);
    let address = cursor.ins().stack_addr(types::I64, slot, 0);
    cursor.ins().store(MemFlags::new(), argument, address, 0);
    cursor.func.dfg.inst_args_mut(call)[0] = address;
    (function, points)
}

/// Run the inliner over `caller` and `callee`, returning both as they are
/// handed to be defined.
fn inline_pair(
    caller: (Function, SourcePoints),
    callee: (Function, SourcePoints),
) -> (TranslatedFunction<'static>, TranslatedFunction<'static>) {
    let options = CraneliftOptions {
        opt_level: OptLevel::Speed,
        ..CraneliftOptions::default()
    };
    let isa = build_isa(target_lexicon::Triple::host(), &options)
        .unwrap_or_else(|e| panic!("the host target builds: {e}"));
    let functions = vec![
        translated("miri.caller", 0, caller),
        translated("miri.callee", 1, callee),
    ];
    let mut defined = Vec::new();
    inline_and_define(functions, &*isa, |_, function| {
        defined.push(function);
        Ok(())
    })
    .unwrap_or_else(|e| panic!("inlining succeeds: {e}"));
    let caller = defined
        .pop()
        .unwrap_or_else(|| panic!("the caller was defined"));
    let callee = defined
        .pop()
        .unwrap_or_else(|| panic!("the callee was defined"));
    (caller, callee)
}

fn has_opcode(function: &Function, opcode: Opcode) -> bool {
    (function.layout.blocks()).any(|block| {
        (function.layout.block_insts(block)).any(|inst| function.dfg.insts[inst].opcode() == opcode)
    })
}

#[test]
fn a_callee_is_weighed_by_its_stack_after_its_slots_are_promoted() {
    let size = u32::try_from(ADDED_STACK_LIMIT + 8)
        .unwrap_or_else(|e| panic!("the stack limit fits a slot: {e}"));
    let (caller, callee) = inline_pair(caller(types::I64), slot_holding_callee(size));
    assert!(
        !has_call(&caller.function),
        "a callee whose slot is promoted away was not inlined:\n{}",
        caller.function
    );
    let sizes: Vec<u32> = callee
        .function
        .sized_stack_slots
        .values()
        .map(|s| s.size)
        .collect();
    assert_eq!(sizes, vec![0], "{}", callee.function);
}

#[test]
fn a_slot_reached_through_an_inlined_callee_is_promoted_and_the_callee_keeps_its_points() {
    let (caller, _) = inline_pair(slot_passing_caller(), reading_callee());
    let function = &caller.function;
    assert!(!has_call(function), "the call was not inlined:\n{function}");
    for opcode in [Opcode::Load, Opcode::Store, Opcode::StackAddr] {
        assert!(
            !has_opcode(function, opcode),
            "{opcode} left in:\n{function}"
        );
    }
    let points: Vec<Option<SourcePoint>> = (function.layout.blocks())
        .flat_map(|block| function.layout.block_insts(block))
        .map(|inst| caller.source_points.point_at(function.srcloc(inst)))
        .collect();
    let inlined = SourcePoint {
        source: SourceId::new(1),
        offset: 20,
        inlined_at: Some(0),
    };
    assert!(
        points.contains(&Some(inlined)),
        "the callee's addition lost its point: {points:?}"
    );
}
