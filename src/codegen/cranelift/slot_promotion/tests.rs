// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Slot promotion over hand-built CLIF: which slots it empties, which it
//! leaves alone, and the SSA form it leaves behind.

use super::super::{build_isa, CraneliftOptions, OptLevel};
use super::*;
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    types, AbiParam, Block, ExtFuncData, ExternalName, InstBuilder, JumpTableData, MemFlags,
    Opcode, Signature, SourceLoc, StackSlot, StackSlotData, Type, UserExternalName, UserFuncName,
    Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use std::sync::Arc;

fn host_isa() -> Arc<dyn TargetIsa> {
    let options = CraneliftOptions {
        opt_level: OptLevel::Speed,
        ..CraneliftOptions::default()
    };
    build_isa(target_lexicon::Triple::host(), &options)
        .unwrap_or_else(|e| panic!("the host target builds: {e}"))
}

fn signature(params: &[Type], returns: &[Type]) -> Signature {
    let mut signature = Signature::new(CallConv::triple_default(&target_lexicon::Triple::host()));
    signature
        .params
        .extend(params.iter().map(|&ty| AbiParam::new(ty)));
    signature
        .returns
        .extend(returns.iter().map(|&ty| AbiParam::new(ty)));
    signature
}

/// A function of `params` returning `returns`, its body built by `body`
/// from the entry block on.
fn function(
    params: &[Type],
    returns: &[Type],
    body: impl FnOnce(&mut FunctionBuilder<'_>, Block),
) -> Function {
    let mut function =
        Function::with_name_signature(UserFuncName::default(), signature(params, returns));
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    body(&mut builder, entry);
    builder.seal_all_blocks();
    builder.finalize();
    function
}

fn slot(builder: &mut FunctionBuilder<'_>, size: u32) -> StackSlot {
    builder.create_sized_stack_slot(StackSlotData::new(StackSlotKind::ExplicitSlot, size, 3))
}

fn entry_of(function: &Function) -> Block {
    (function.layout.entry_block()).unwrap_or_else(|| panic!("no entry block in:\n{function}"))
}

fn param(builder: &FunctionBuilder<'_>, block: Block, index: usize) -> Value {
    builder.block_params(block)[index]
}

/// Promote `function`'s slots, checking the result verifies.
fn promoted(function: &mut Function) -> Promotion {
    let isa = host_isa();
    let promotion = promote(&mut Context::new(), function, &*isa)
        .unwrap_or_else(|e| panic!("promotion succeeds: {e}"));
    if let Err(errors) = cranelift_codegen::verify_function(function, &*isa) {
        panic!("the promoted function does not verify: {errors}\n{function}");
    }
    promotion
}

/// `fn(f64, f64, f64) f64` storing its three arguments as the fields of a
/// 24-byte value and returning their sum read back.
fn three_field_value() -> Function {
    let float = types::F64;
    function(&[float, float, float], &[float], |builder, entry| {
        let value = slot(builder, 24);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        for field in 0..3 {
            let argument = param(builder, entry, field);
            let offset = 8 * field as i32;
            builder
                .ins()
                .store(MemFlags::new(), argument, address, offset);
        }
        let x = builder.ins().load(float, MemFlags::new(), address, 0);
        let y = builder.ins().load(float, MemFlags::new(), address, 8);
        let z = builder.ins().load(float, MemFlags::new(), address, 16);
        let sum = builder.ins().fadd(x, y);
        let sum = builder.ins().fadd(sum, z);
        builder.ins().return_(&[sum]);
    })
}

/// `fn(i64) i64` storing its argument through `address(builder, slot)`
/// into an 8-byte slot and returning it read back.
fn stored_through(
    address: impl FnOnce(&mut FunctionBuilder<'_>, StackSlot, Value) -> Value,
) -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = address(builder, value, argument);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        let read = builder.ins().load(types::I64, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    })
}

#[test]
fn a_value_built_and_read_field_by_field_is_promoted_cell_by_cell() {
    let mut function = three_field_value();
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 3 });
}

#[test]
fn an_address_offset_by_an_added_constant_reaches_the_cell_it_names() {
    let mut legalized = stored_through(|builder, value, _| {
        let base = builder.ins().stack_addr(types::I64, value, 0);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().iadd(base, zero)
    });
    assert_eq!(promoted(&mut legalized), Promotion { slots: 1, cells: 1 });
    let mut swapped = stored_through(|builder, value, _| {
        let base = builder.ins().stack_addr(types::I64, value, 4);
        let back = builder.ins().iconst(types::I64, -4);
        builder.ins().iadd(back, base)
    });
    assert_eq!(promoted(&mut swapped), Promotion { slots: 1, cells: 1 });
}

#[test]
fn a_slot_named_by_stack_load_and_stack_store_is_promoted() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 0);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
}

#[test]
fn an_address_offset_by_a_value_known_only_at_run_time_keeps_its_slot() {
    let mut function = stored_through(|builder, value, argument| {
        let base = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().iadd(base, argument)
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

/// A function storing an `i64` into an 8-byte slot and handing the slot's
/// address to `escape`.
fn escaping(escape: impl FnOnce(&mut FunctionBuilder<'_>, Value)) -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        escape(builder, address);
        let read = builder.ins().load(types::I64, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    })
}

#[test]
fn a_slot_whose_address_is_passed_to_a_call_is_kept() {
    let mut function = escaping(|builder, address| {
        let name = builder
            .func
            .declare_imported_user_function(UserExternalName::new(0, 1));
        let signature = builder.import_signature(signature(&[types::I64], &[]));
        let callee = builder.import_function(ExtFuncData {
            name: ExternalName::User(name),
            signature,
            colocated: false,
        });
        builder.ins().call(callee, &[address]);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

#[test]
fn a_slot_whose_address_is_stored_as_data_is_kept() {
    let mut function = escaping(|builder, address| {
        let holder = slot(builder, 8);
        builder.ins().stack_store(address, holder, 0);
    });
    let promotion = promoted(&mut function);
    assert_eq!(promotion, Promotion { slots: 1, cells: 1 }, "{function}");
    assert_eq!(
        function.sized_stack_slots.values().next().map(|s| s.size),
        Some(8)
    );
}

#[test]
fn a_slot_whose_address_is_passed_to_a_block_is_kept() {
    let mut function = escaping(|builder, address| {
        let next = builder.create_block();
        builder.append_block_param(next, types::I64);
        builder.ins().jump(next, &[address.into()]);
        builder.switch_to_block(next);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

#[test]
fn a_slot_whose_address_is_compared_is_kept() {
    let mut function = escaping(|builder, address| {
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().icmp(IntCC::Equal, address, zero);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

#[test]
fn a_slot_read_whole_after_being_written_in_halves_is_kept() {
    let mut function = function(&[types::I32], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        builder.ins().store(MemFlags::new(), argument, address, 4);
        let read = builder.ins().load(types::I64, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

#[test]
fn a_slot_accessed_past_its_end_is_kept() {
    let mut past_end =
        stored_through(|builder, value, _| builder.ins().stack_addr(types::I64, value, 4));
    assert_eq!(promoted(&mut past_end), Promotion::default());
    let mut before_start = stored_through(|builder, value, _| {
        let base = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().iadd_imm(base, -8)
    });
    assert_eq!(promoted(&mut before_start), Promotion::default());
}

#[test]
fn a_slot_read_by_an_extending_load_is_kept() {
    let mut function = function(&[types::I8], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        let read = builder
            .ins()
            .uload8(types::I64, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

#[test]
fn a_slot_written_by_a_truncating_store_is_kept() {
    let mut function = function(&[types::I64], &[types::I8], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().istore8(MemFlags::new(), argument, address, 0);
        let read = builder.ins().load(types::I8, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

#[test]
fn a_slot_accessed_with_an_explicit_byte_order_is_kept() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        let big = MemFlags::new().with_endianness(cranelift_codegen::ir::Endianness::Big);
        builder.ins().store(big, argument, address, 0);
        let read = builder.ins().load(types::I64, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

/// How many instructions of `function`'s layout are `opcode`.
fn count(function: &Function, opcode: Opcode) -> usize {
    (function.layout.blocks())
        .flat_map(|block| function.layout.block_insts(block))
        .filter(|&inst| function.dfg.insts[inst].opcode() == opcode)
        .count()
}

/// Whether `function` reaches memory or a slot's address at all.
fn touches_memory(function: &Function) -> bool {
    [
        Opcode::Load,
        Opcode::Store,
        Opcode::StackLoad,
        Opcode::StackStore,
        Opcode::StackAddr,
    ]
    .into_iter()
    .any(|opcode| count(function, opcode) != 0)
}

fn slot_sizes(function: &Function) -> Vec<u32> {
    function
        .sized_stack_slots
        .values()
        .map(|slot| slot.size)
        .collect()
}

/// The values each branch to `block` hands it.
fn arguments_to(function: &Function, block: Block) -> Vec<Vec<Value>> {
    let dfg = &function.dfg;
    (function.layout.blocks())
        .filter_map(|from| function.layout.last_inst(from))
        .flat_map(|inst| {
            dfg.insts[inst].branch_destination(&dfg.jump_tables, &dfg.exception_tables)
        })
        .filter(|call| call.block(&dfg.value_lists) == block)
        .map(|call| {
            (call.args(&dfg.value_lists))
                .filter_map(|arg| arg.as_value())
                .map(|value| dfg.resolve_aliases(value))
                .collect()
        })
        .collect()
}

#[test]
fn a_promoted_value_leaves_no_access_of_its_slot_and_an_empty_slot() {
    let mut function = three_field_value();
    promoted(&mut function);
    assert!(!touches_memory(&function), "{function}");
    assert_eq!(slot_sizes(&function), vec![0], "{function}");
}

/// `fn(i64 condition, i64, i64) i64` storing its second argument on one arm
/// of a branch and its third on the other, then returning what was stored.
fn diamond() -> (Function, Block) {
    let mut join = None;
    let function = function(&[types::I64; 3], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (then_arm, else_arm, after) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, then_arm, &[], else_arm, &[]);
        for (arm, index) in [(then_arm, 1), (else_arm, 2)] {
            builder.switch_to_block(arm);
            let argument = param(builder, entry, index);
            builder.ins().stack_store(argument, value, 0);
            builder.ins().jump(after, &[]);
        }
        builder.switch_to_block(after);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
        join = Some(after);
    });
    (
        function,
        join.unwrap_or_else(|| panic!("the join was built")),
    )
}

#[test]
fn a_cell_written_on_both_arms_of_a_branch_joins_in_a_block_parameter() {
    let (mut function, join) = diamond();
    let entry = entry_of(&function);
    let arguments: Vec<Value> = function.dfg.block_params(entry).to_vec();
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert!(!touches_memory(&function), "{function}");
    assert_eq!(function.dfg.block_params(join).len(), 1, "{function}");
    let mut handed = arguments_to(&function, join);
    handed.sort();
    assert_eq!(
        handed,
        vec![vec![arguments[1]], vec![arguments[2]]],
        "{function}"
    );
}

#[test]
fn a_cell_written_on_one_arm_only_and_read_after_the_join_is_kept() {
    let mut function = function(&[types::I64; 2], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (arm, after) = (builder.create_block(), builder.create_block());
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, arm, &[], after, &[]);
        builder.switch_to_block(arm);
        let argument = param(builder, entry, 1);
        builder.ins().stack_store(argument, value, 0);
        builder.ins().jump(after, &[]);
        builder.switch_to_block(after);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
    });
    let before = function.display().to_string();
    assert_eq!(promoted(&mut function), Promotion::default());
    assert_eq!(function.display().to_string(), before);
}

#[test]
fn a_cell_read_before_it_is_written_is_kept() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let read = builder.ins().stack_load(types::I64, value, 0);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 0);
        builder.ins().return_(&[read]);
    });
    let before = function.display().to_string();
    assert_eq!(promoted(&mut function), Promotion::default());
    assert_eq!(function.display().to_string(), before);
}

#[test]
fn a_cell_written_in_a_loop_body_reaches_its_header_along_the_back_edge() {
    let mut header = None;
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let counter = slot(builder, 8);
        let (top, body, exit) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().stack_store(zero, counter, 0);
        builder.ins().jump(top, &[]);
        builder.switch_to_block(top);
        let count = builder.ins().stack_load(types::I64, counter, 0);
        let limit = param(builder, entry, 0);
        let more = builder.ins().icmp(IntCC::SignedLessThan, count, limit);
        builder.ins().brif(more, body, &[], exit, &[]);
        builder.switch_to_block(body);
        let next = builder.ins().iadd_imm(count, 1);
        builder.ins().stack_store(next, counter, 0);
        builder.ins().jump(top, &[]);
        builder.switch_to_block(exit);
        builder.ins().return_(&[count]);
        header = Some((top, zero));
    });
    let (top, zero) = header.unwrap_or_else(|| panic!("the loop was built"));
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert!(!touches_memory(&function), "{function}");
    let params = function.dfg.block_params(top).to_vec();
    assert_eq!(params.len(), 1, "{function}");
    let handed = arguments_to(&function, top);
    assert_eq!(handed.len(), 2, "{function}");
    assert!(handed.contains(&vec![zero]), "{function}");
    assert!(handed
        .iter()
        .all(|values| values.len() == 1 && values[0] != params[0]));
}

#[test]
fn a_float_written_and_read_back_as_an_integer_costs_one_bitcast() {
    let mut function = function(&[types::F64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 0);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert!(!touches_memory(&function), "{function}");
    assert_eq!(count(&function, Opcode::Bitcast), 1, "{function}");
}

#[test]
fn a_float_copied_through_an_integer_cell_needs_no_bitcast() {
    let mut function = function(&[types::F64], &[types::F64], |builder, entry| {
        let (first, second) = (slot(builder, 8), slot(builder, 8));
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, first, 0);
        let word = builder.ins().stack_load(types::I64, first, 0);
        builder.ins().stack_store(word, second, 0);
        let read = builder.ins().stack_load(types::F64, second, 0);
        builder.ins().return_(&[read]);
    });
    let argument = function.dfg.block_params(entry_of(&function))[0];
    assert_eq!(promoted(&mut function), Promotion { slots: 2, cells: 2 });
    assert!(!touches_memory(&function), "{function}");
    assert_eq!(count(&function, Opcode::Bitcast), 0, "{function}");
    let returned = (function.layout.last_block())
        .and_then(|exit| function.layout.last_inst(exit))
        .unwrap_or_else(|| panic!("no return in:\n{function}"));
    assert_eq!(
        function
            .dfg
            .resolve_aliases(function.dfg.inst_args(returned)[0]),
        argument
    );
}

#[test]
fn an_integer_zero_written_to_a_float_cell_becomes_a_float_constant() {
    let mut function = function(&[], &[types::F64], |builder, _| {
        let value = slot(builder, 8);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().stack_store(zero, value, 0);
        let read = builder.ins().stack_load(types::F64, value, 0);
        builder.ins().return_(&[read]);
    });
    promoted(&mut function);
    assert!(!touches_memory(&function), "{function}");
    assert_eq!(count(&function, Opcode::Bitcast), 0, "{function}");
    assert_eq!(count(&function, Opcode::F64const), 1, "{function}");
}

#[test]
fn a_bitcast_takes_the_source_location_of_the_access_it_replaces() {
    let at = SourceLoc::new(7);
    let mut function = function(&[types::F64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 0);
        builder.set_srcloc(at);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.set_srcloc(SourceLoc::default());
        builder.ins().return_(&[read]);
    });
    promoted(&mut function);
    let bitcast = (function.layout.blocks())
        .flat_map(|block| function.layout.block_insts(block))
        .find(|&inst| function.dfg.insts[inst].opcode() == Opcode::Bitcast)
        .unwrap_or_else(|| panic!("no bitcast in:\n{function}"));
    assert_eq!(function.srcloc(bitcast), at);
}

#[test]
fn promotion_repeats_exactly_and_finds_nothing_more_the_second_time() {
    let promote_text = || {
        let (mut function, _) = diamond();
        promoted(&mut function);
        function
    };
    let mut first = promote_text();
    let text = first.display().to_string();
    assert_eq!(promote_text().display().to_string(), text);
    assert_eq!(promoted(&mut first), Promotion::default());
    assert_eq!(first.display().to_string(), text);
}

#[test]
fn branch_tables_sharing_a_jump_table_each_hand_their_own_values() {
    let mut join = None;
    let mut function = function(&[types::I64; 3], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (left, right, after) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let condition = param(builder, entry, 0);
        let selector = builder.ins().ireduce(types::I32, condition);
        builder.ins().brif(condition, left, &[], right, &[]);
        let target = builder.func.dfg.block_call(after, &[]);
        let entries = [builder.func.dfg.block_call(after, &[])];
        let table = builder.create_jump_table(JumpTableData::new(target, &entries));
        for (arm, index) in [(left, 1), (right, 2)] {
            builder.switch_to_block(arm);
            let argument = param(builder, entry, index);
            builder.ins().stack_store(argument, value, 0);
            builder.ins().br_table(selector, table);
        }
        builder.switch_to_block(after);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
        join = Some(after);
    });
    let join = join.unwrap_or_else(|| panic!("the join was built"));
    let arguments: Vec<Value> = function.dfg.block_params(entry_of(&function)).to_vec();
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    let mut handed = arguments_to(&function, join);
    handed.sort();
    handed.dedup();
    assert_eq!(
        handed,
        vec![vec![arguments[1]], vec![arguments[2]]],
        "{function}"
    );
}
