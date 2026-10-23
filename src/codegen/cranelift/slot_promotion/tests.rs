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
    UserStackMapEntry, Value, ValueDef,
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
fn a_slot_whose_address_is_stored_as_data_is_kept_and_the_slot_holding_it_is_promoted() {
    let mut holder = None;
    let mut function = escaping(|builder, address| {
        let slot = slot(builder, 8);
        builder.ins().stack_store(address, slot, 0);
        holder = Some(slot);
    });
    let holder = holder.unwrap_or_else(|| panic!("the holder was built"));
    let value = StackSlot::from_u32(0);
    let promotion = promoted(&mut function);
    assert_eq!(promotion, Promotion { slots: 1, cells: 1 }, "{function}");
    assert_eq!(function.sized_stack_slots[value].size, 8, "{function}");
    assert_eq!(function.sized_stack_slots[holder].size, 0, "{function}");
}

#[test]
fn a_slot_whose_address_joins_another_value_at_a_block_is_kept() {
    let mut function = escaping(|builder, address| {
        let next = builder.create_block();
        builder.append_block_param(next, types::I64);
        let other = builder.ins().iconst(types::I64, 0);
        let condition = builder.ins().iconst(types::I64, 1);
        builder
            .ins()
            .brif(condition, next, &[address.into()], next, &[other.into()]);
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
        header = Some((top, zero, next));
    });
    let (top, zero, next) = header.unwrap_or_else(|| panic!("the loop was built"));
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert!(!touches_memory(&function), "{function}");
    let params = function.dfg.block_params(top).to_vec();
    assert_eq!(params.len(), 1, "{function}");
    let handed = arguments_to(&function, top);
    assert_eq!(handed.len(), 2, "{function}");
    assert!(handed.contains(&vec![zero]), "{function}");
    assert!(handed.contains(&vec![next]), "{function}");
    assert_eq!(operand(&function, next), params[0], "{function}");
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
    let expected = vec![
        vec![arguments[1]],
        vec![arguments[1]],
        vec![arguments[2]],
        vec![arguments[2]],
    ];
    assert_eq!(handed, expected, "{function}");
}

/// The first operand of the instruction defining `value`, its aliases
/// resolved.
fn operand(function: &Function, value: Value) -> Value {
    let ValueDef::Result(inst, _) = function.dfg.value_def(function.dfg.resolve_aliases(value))
    else {
        panic!("{value} is not an instruction result in:\n{function}");
    };
    function
        .dfg
        .resolve_aliases(function.dfg.inst_args(inst)[0])
}

/// The value `function` returns, its aliases resolved.
fn returned(function: &Function) -> Value {
    let exit = (function.layout.blocks())
        .filter_map(|block| function.layout.last_inst(block))
        .find(|&inst| function.dfg.insts[inst].opcode() == Opcode::Return)
        .unwrap_or_else(|| panic!("no return in:\n{function}"));
    function
        .dfg
        .resolve_aliases(function.dfg.inst_args(exit)[0])
}

/// The values each branch to `block` hands it, with the block it is from,
/// in layout order and, within a branch, in destination order.
fn edges_into(function: &Function, block: Block) -> Vec<(Block, Vec<Value>)> {
    let dfg = &function.dfg;
    let mut edges = Vec::new();
    for from in function.layout.blocks() {
        let Some(branch) = function.layout.last_inst(from) else {
            continue;
        };
        for call in dfg.insts[branch].branch_destination(&dfg.jump_tables, &dfg.exception_tables) {
            if call.block(&dfg.value_lists) != block {
                continue;
            }
            let values = (call.args(&dfg.value_lists))
                .filter_map(|arg| arg.as_value())
                .map(|value| dfg.resolve_aliases(value))
                .collect();
            edges.push((from, values));
        }
    }
    edges
}

#[test]
fn a_loop_with_two_latches_hands_its_header_each_latchs_value() {
    let mut built = None;
    let mut function = function(
        &[types::I64, types::I64],
        &[types::I64],
        |builder, entry| {
            let counter = slot(builder, 8);
            let blocks: Vec<Block> = (0..5).map(|_| builder.create_block()).collect();
            let (top, body, first, second, exit) =
                (blocks[0], blocks[1], blocks[2], blocks[3], blocks[4]);
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().stack_store(zero, counter, 0);
            builder.ins().jump(top, &[]);
            builder.switch_to_block(top);
            let count = builder.ins().stack_load(types::I64, counter, 0);
            let limit = param(builder, entry, 0);
            let more = builder.ins().icmp(IntCC::SignedLessThan, count, limit);
            builder.ins().brif(more, body, &[], exit, &[]);
            builder.switch_to_block(body);
            let choice = param(builder, entry, 1);
            builder.ins().brif(choice, first, &[], second, &[]);
            let mut steps = Vec::new();
            for (latch, step) in [(first, 1), (second, 2)] {
                builder.switch_to_block(latch);
                let next = builder.ins().iadd_imm(count, step);
                builder.ins().stack_store(next, counter, 0);
                builder.ins().jump(top, &[]);
                steps.push(next);
            }
            builder.switch_to_block(exit);
            let last = builder.ins().stack_load(types::I64, counter, 0);
            builder.ins().return_(&[last]);
            built = Some((blocks, entry, zero, steps));
        },
    );
    let (blocks, entry, zero, steps) = built.unwrap_or_else(|| panic!("the loop was built"));
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert!(!touches_memory(&function), "{function}");
    let header = function.dfg.block_params(blocks[0]).to_vec();
    assert_eq!(header.len(), 1, "{function}");
    let expected = vec![
        (entry, vec![zero]),
        (blocks[2], vec![steps[0]]),
        (blocks[3], vec![steps[1]]),
    ];
    assert_eq!(edges_into(&function, blocks[0]), expected, "{function}");
    for step in steps {
        assert_eq!(operand(&function, step), header[0], "{function}");
    }
    assert_eq!(returned(&function), header[0], "{function}");
}

#[test]
fn nested_loops_take_a_parameter_for_each_cell_live_into_their_headers() {
    let mut headers = None;
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let state = slot(builder, 24);
        let (i, sum, j) = (0, 8, 16);
        let blocks: Vec<Block> = (0..6).map(|_| builder.create_block()).collect();
        let (outer, inner_start, inner, inner_body, outer_latch, exit) = (
            blocks[0], blocks[1], blocks[2], blocks[3], blocks[4], blocks[5],
        );
        let limit = param(builder, entry, 0);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().stack_store(zero, state, i);
        builder.ins().stack_store(zero, state, sum);
        builder.ins().jump(outer, &[]);
        builder.switch_to_block(outer);
        let row = builder.ins().stack_load(types::I64, state, i);
        let more_rows = builder.ins().icmp(IntCC::SignedLessThan, row, limit);
        builder.ins().brif(more_rows, inner_start, &[], exit, &[]);
        builder.switch_to_block(inner_start);
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().stack_store(zero, state, j);
        builder.ins().jump(inner, &[]);
        builder.switch_to_block(inner);
        let column = builder.ins().stack_load(types::I64, state, j);
        let more_columns = builder.ins().icmp(IntCC::SignedLessThan, column, limit);
        builder
            .ins()
            .brif(more_columns, inner_body, &[], outer_latch, &[]);
        builder.switch_to_block(inner_body);
        let total = builder.ins().stack_load(types::I64, state, sum);
        let total = builder.ins().iadd(total, column);
        builder.ins().stack_store(total, state, sum);
        let column = builder.ins().iadd_imm(column, 1);
        builder.ins().stack_store(column, state, j);
        builder.ins().jump(inner, &[]);
        builder.switch_to_block(outer_latch);
        let row = builder.ins().stack_load(types::I64, state, i);
        let row = builder.ins().iadd_imm(row, 1);
        builder.ins().stack_store(row, state, i);
        builder.ins().jump(outer, &[]);
        builder.switch_to_block(exit);
        let total = builder.ins().stack_load(types::I64, state, sum);
        builder.ins().return_(&[total]);
        headers = Some((outer, inner));
    });
    let (outer, inner) = headers.unwrap_or_else(|| panic!("the loops were built"));
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 3 });
    assert!(!touches_memory(&function), "{function}");
    assert_eq!(function.dfg.block_params(outer).len(), 2, "{function}");
    assert_eq!(function.dfg.block_params(inner).len(), 3, "{function}");
}

#[test]
fn a_critical_edge_hands_the_value_its_source_block_wrote() {
    let mut built = None;
    let mut function = function(&[types::I64; 3], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (other, join) = (builder.create_block(), builder.create_block());
        let first = param(builder, entry, 1);
        builder.ins().stack_store(first, value, 0);
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, join, &[], other, &[]);
        builder.switch_to_block(other);
        let second = param(builder, entry, 2);
        builder.ins().stack_store(second, value, 0);
        builder.ins().jump(join, &[]);
        builder.switch_to_block(join);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
        built = Some((entry, other, join, first, second));
    });
    let (entry, other, join, first, second) = built.unwrap_or_else(|| panic!("built"));
    promoted(&mut function);
    let expected = vec![(entry, vec![first]), (other, vec![second])];
    assert_eq!(edges_into(&function, join), expected, "{function}");
}

#[test]
fn a_branch_to_one_block_on_both_arms_counts_as_one_predecessor() {
    let mut function = function(&[types::I64; 2], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let next = builder.create_block();
        let argument = param(builder, entry, 1);
        builder.ins().stack_store(argument, value, 0);
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, next, &[], next, &[]);
        builder.switch_to_block(next);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
    });
    let argument = function.dfg.block_params(entry_of(&function))[1];
    promoted(&mut function);
    let next = function
        .layout
        .last_block()
        .unwrap_or_else(|| panic!("no block"));
    assert!(function.dfg.block_params(next).is_empty(), "{function}");
    assert_eq!(returned(&function), argument, "{function}");
}

#[test]
fn a_branch_to_one_block_on_both_arms_hands_the_value_on_each_arm() {
    let mut built = None;
    let mut function = function(&[types::I64; 4], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (left, right, join) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, left, &[], right, &[]);
        builder.switch_to_block(left);
        let first = param(builder, entry, 2);
        builder.ins().stack_store(first, value, 0);
        let again = param(builder, entry, 1);
        builder.ins().brif(again, join, &[], join, &[]);
        builder.switch_to_block(right);
        let second = param(builder, entry, 3);
        builder.ins().stack_store(second, value, 0);
        builder.ins().jump(join, &[]);
        builder.switch_to_block(join);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
        built = Some((left, right, join, first, second));
    });
    let (left, right, join, first, second) = built.unwrap_or_else(|| panic!("built"));
    promoted(&mut function);
    let expected = vec![
        (left, vec![first]),
        (left, vec![first]),
        (right, vec![second]),
    ];
    assert_eq!(edges_into(&function, join), expected, "{function}");
}

#[test]
fn a_join_with_parameters_of_its_own_takes_the_cells_after_them() {
    let mut built = None;
    let mut function = function(&[types::I64; 3], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (left, right, join) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let own = builder.append_block_param(join, types::I64);
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, left, &[], right, &[]);
        let mut handed = Vec::new();
        for (arm, index, tag) in [(left, 1, 10), (right, 2, 20)] {
            builder.switch_to_block(arm);
            let argument = param(builder, entry, index);
            builder.ins().stack_store(argument, value, 0);
            let tag = builder.ins().iconst(types::I64, tag);
            builder.ins().jump(join, &[tag.into()]);
            handed.push((arm, vec![tag, argument]));
        }
        builder.switch_to_block(join);
        let read = builder.ins().stack_load(types::I64, value, 0);
        let sum = builder.ins().iadd(own, read);
        builder.ins().return_(&[sum]);
        built = Some((join, own, handed));
    });
    let (join, own, handed) = built.unwrap_or_else(|| panic!("built"));
    promoted(&mut function);
    let params = function.dfg.block_params(join).to_vec();
    assert_eq!(params.len(), 2, "{function}");
    assert_eq!(params[0], own, "{function}");
    assert_eq!(edges_into(&function, join), handed, "{function}");
}

#[test]
fn a_branch_table_hands_each_of_its_edges_the_value_its_block_wrote() {
    let mut built = None;
    let mut function = function(&[types::I64; 3], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let blocks: Vec<Block> = (0..4).map(|_| builder.create_block()).collect();
        let (table_block, other, join, single) = (blocks[0], blocks[1], blocks[2], blocks[3]);
        let condition = param(builder, entry, 0);
        builder.ins().brif(condition, table_block, &[], other, &[]);
        builder.switch_to_block(table_block);
        let first = param(builder, entry, 1);
        builder.ins().stack_store(first, value, 0);
        let default = builder.func.dfg.block_call(join, &[]);
        let entries: Vec<_> = [join, single, join]
            .into_iter()
            .map(|target| builder.func.dfg.block_call(target, &[]))
            .collect();
        let table = builder.create_jump_table(JumpTableData::new(default, &entries));
        let selector = builder.ins().ireduce(types::I32, condition);
        builder.ins().br_table(selector, table);
        builder.switch_to_block(other);
        let second = param(builder, entry, 2);
        builder.ins().stack_store(second, value, 0);
        builder.ins().jump(join, &[]);
        for target in [join, single] {
            builder.switch_to_block(target);
            let read = builder.ins().stack_load(types::I64, value, 0);
            builder.ins().return_(&[read]);
        }
        built = Some((table_block, other, join, single, first, second));
    });
    let (table_block, other, join, single, first, second) =
        built.unwrap_or_else(|| panic!("built"));
    promoted(&mut function);
    let expected = vec![
        (table_block, vec![first]),
        (table_block, vec![first]),
        (table_block, vec![first]),
        (other, vec![second]),
    ];
    assert_eq!(edges_into(&function, join), expected, "{function}");
    assert_eq!(
        edges_into(&function, single),
        vec![(table_block, vec![])],
        "{function}"
    );
    assert!(function.dfg.block_params(single).is_empty(), "{function}");
}

/// A function keeping a 16-byte slot whose first cell is read before it is
/// written, created before an 8-byte slot it promotes, returning the
/// second slot's value.
fn kept_slot_before_promoted_one() -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let kept = slot(builder, 16);
        let promoted_slot = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let early = builder.ins().stack_load(types::I64, kept, 0);
        builder.ins().stack_store(argument, kept, 0);
        builder.ins().stack_store(early, kept, 8);
        builder.ins().stack_store(argument, promoted_slot, 0);
        let read = builder.ins().stack_load(types::I64, promoted_slot, 0);
        builder.ins().return_(&[read]);
    })
}

#[test]
fn a_slot_kept_for_a_read_before_its_write_leaves_a_later_slot_promoted() {
    let mut function = kept_slot_before_promoted_one();
    let argument = function.dfg.block_params(entry_of(&function))[0];
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert_eq!(slot_sizes(&function), vec![16, 0], "{function}");
    assert_eq!(count(&function, Opcode::StackLoad), 1, "{function}");
    assert_eq!(count(&function, Opcode::StackStore), 2, "{function}");
    assert_eq!(returned(&function), argument, "{function}");
}

#[test]
fn a_slot_kept_for_overlapping_accesses_leaves_a_later_slot_promoted() {
    let mut function = function(
        &[types::I32, types::I64],
        &[types::I64],
        |builder, entry| {
            let kept = slot(builder, 8);
            let promoted_slot = slot(builder, 8);
            let half = param(builder, entry, 0);
            builder.ins().stack_store(half, kept, 0);
            builder.ins().stack_store(half, kept, 4);
            let whole = builder.ins().stack_load(types::I64, kept, 0);
            builder.ins().stack_store(whole, promoted_slot, 0);
            let read = builder.ins().stack_load(types::I64, promoted_slot, 0);
            builder.ins().return_(&[read]);
        },
    );
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert_eq!(slot_sizes(&function), vec![8, 0], "{function}");
    assert_eq!(count(&function, Opcode::StackLoad), 1, "{function}");
}

#[test]
fn a_second_promotion_of_a_function_keeping_a_slot_changes_nothing() {
    let mut function = kept_slot_before_promoted_one();
    promoted(&mut function);
    let text = function.display().to_string();
    assert_eq!(promoted(&mut function), Promotion::default());
    assert_eq!(function.display().to_string(), text);
}

/// A function reading its slot in blocks `dead` builds, which its entry
/// never reaches.
fn read_where_unreachable(dead: impl FnOnce(&mut FunctionBuilder<'_>, StackSlot)) -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 0);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
        dead(builder, value);
    })
}

#[test]
fn a_cell_read_in_an_unreachable_block_is_promoted_where_it_is_reached() {
    let mut single = read_where_unreachable(|builder, value| {
        let dead = builder.create_block();
        builder.switch_to_block(dead);
        let read = builder.ins().stack_load(types::I64, value, 0);
        builder.ins().return_(&[read]);
    });
    let mut cycle = read_where_unreachable(|builder, value| {
        let (first, second) = (builder.create_block(), builder.create_block());
        for (block, next) in [(first, second), (second, first)] {
            builder.switch_to_block(block);
            let read = builder.ins().stack_load(types::I64, value, 0);
            builder.ins().stack_store(read, value, 0);
            builder.ins().jump(next, &[]);
        }
    });
    for function in [&mut single, &mut cycle] {
        let argument = function.dfg.block_params(entry_of(function))[0];
        assert_eq!(promoted(function), Promotion { slots: 1, cells: 1 });
        assert!(!touches_memory(function), "{function}");
        assert_eq!(returned(function), argument, "{function}");
    }
}

/// `fn(from) to` storing its argument as `from` and reading it back as `to`.
fn reinterpreted(from: Type, to: Type) -> Function {
    function(&[from], &[to], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 0);
        let read = builder.ins().stack_load(to, value, 0);
        builder.ins().return_(&[read]);
    })
}

#[test]
fn a_32_bit_float_and_integer_convert_either_way_by_one_bitcast() {
    for (from, to) in [(types::F32, types::I32), (types::I32, types::F32)] {
        let mut function = reinterpreted(from, to);
        assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
        assert!(!touches_memory(&function), "{function}");
        assert_eq!(
            count(&function, Opcode::Bitcast),
            1,
            "{from} to {to}:\n{function}"
        );
    }
}

#[test]
fn a_cell_read_and_written_at_two_types_crosses_a_back_edge_as_the_float() {
    let mut header = None;
    let mut function = function(
        &[types::F64, types::I64],
        &[types::F64],
        |builder, entry| {
            let value = slot(builder, 8);
            let (top, body, exit) = (
                builder.create_block(),
                builder.create_block(),
                builder.create_block(),
            );
            let start = param(builder, entry, 0);
            builder.ins().stack_store(start, value, 0);
            builder.ins().jump(top, &[]);
            builder.switch_to_block(top);
            let bits = builder.ins().stack_load(types::I64, value, 0);
            let limit = param(builder, entry, 1);
            let more = builder.ins().icmp(IntCC::SignedLessThan, bits, limit);
            builder.ins().brif(more, body, &[], exit, &[]);
            builder.switch_to_block(body);
            let next = builder.ins().iadd_imm(bits, 1);
            builder.ins().stack_store(next, value, 0);
            builder.ins().jump(top, &[]);
            builder.switch_to_block(exit);
            let read = builder.ins().stack_load(types::F64, value, 0);
            builder.ins().return_(&[read]);
            header = Some(top);
        },
    );
    let top = header.unwrap_or_else(|| panic!("built"));
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert!(!touches_memory(&function), "{function}");
    let params = function.dfg.block_params(top).to_vec();
    assert_eq!(params.len(), 1, "{function}");
    assert_eq!(function.dfg.value_type(params[0]), types::F64, "{function}");
    assert_eq!(returned(&function), params[0], "{function}");
}

#[test]
fn a_slot_whose_address_is_returned_is_kept() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        builder.ins().return_(&[address]);
    });
    assert_eq!(promoted(&mut function), Promotion::default());
}

/// Import `fn(i64)` into the function `builder` builds.
fn imported_callee(builder: &mut FunctionBuilder<'_>) -> cranelift_codegen::ir::FuncRef {
    let name = (builder.func).declare_imported_user_function(UserExternalName::new(0, 1));
    let signature = builder.import_signature(signature(&[types::I64], &[]));
    builder.import_function(ExtFuncData {
        name: ExternalName::User(name),
        signature,
        colocated: false,
    })
}

/// An escape of a slot's address into the function `builder` builds.
type Escape = fn(&mut FunctionBuilder<'_>, Value);

#[test]
fn a_slot_whose_address_any_other_instruction_uses_is_kept() {
    let escapes: [(&str, Escape); 6] = [
        ("select", |builder, address| {
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().select(zero, address, zero);
        }),
        ("call_indirect", |builder, address| {
            let signature = builder.import_signature(signature(&[types::I64], &[]));
            let target = builder.ins().iconst(types::I64, 0);
            builder.ins().call_indirect(signature, target, &[address]);
        }),
        ("an offset address passed to a call", |builder, address| {
            let callee = imported_callee(builder);
            let field = builder.ins().iadd_imm(address, 4);
            builder.ins().call(callee, &[field]);
        }),
        (
            "a store of the address through itself",
            |builder, address| {
                builder.ins().store(MemFlags::new(), address, address, 0);
            },
        ),
        ("a call whose stack map names the slot", |builder, _| {
            let callee = imported_callee(builder);
            let zero = builder.ins().iconst(types::I64, 0);
            let call = builder.ins().call(callee, &[zero]);
            let entry = UserStackMapEntry {
                ty: types::I64,
                slot: StackSlot::from_u32(0),
                offset: 0,
            };
            builder.func.dfg.append_user_stack_map_entry(call, entry);
        }),
        ("a debug tag naming the slot", |builder, _| {
            let tagged = builder.ins().sequence_point();
            let tag = cranelift_codegen::ir::DebugTag::StackSlot(StackSlot::from_u32(0));
            builder.func.debug_tags.set(tagged, [tag]);
        }),
    ];
    for (name, escape) in escapes {
        let mut function = escaping(escape);
        assert_eq!(
            promoted(&mut function),
            Promotion::default(),
            "{name}:\n{function}"
        );
    }
}

#[test]
fn a_slot_accessed_atomically_or_at_another_width_is_kept() {
    let escapes: [(&str, Escape); 6] = [
        ("atomic_load", |builder, address| {
            builder
                .ins()
                .atomic_load(types::I64, MemFlags::new(), address);
        }),
        ("atomic_store", |builder, address| {
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().atomic_store(MemFlags::new(), zero, address);
        }),
        ("atomic_rmw", |builder, address| {
            let one = builder.ins().iconst(types::I64, 1);
            let add = cranelift_codegen::ir::AtomicRmwOp::Add;
            builder
                .ins()
                .atomic_rmw(types::I64, MemFlags::new(), add, address, one);
        }),
        ("sload8", |builder, address| {
            builder
                .ins()
                .sload8(types::I64, MemFlags::new(), address, 0);
        }),
        ("uload16", |builder, address| {
            builder
                .ins()
                .uload16(types::I64, MemFlags::new(), address, 0);
        }),
        ("istore16", |builder, address| {
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().istore16(MemFlags::new(), zero, address, 0);
        }),
    ];
    for (name, escape) in escapes {
        let mut function = escaping(escape);
        assert_eq!(
            promoted(&mut function),
            Promotion::default(),
            "{name}:\n{function}"
        );
    }
}

#[test]
fn a_slot_whose_address_is_stored_through_another_slots_address_is_kept() {
    let mut held = None;
    let mut function = escaping(|builder, address| {
        let holder = slot(builder, 8);
        let holder_address = builder.ins().stack_addr(types::I64, holder, 0);
        builder
            .ins()
            .store(MemFlags::new(), address, holder_address, 0);
        held = Some(holder);
    });
    let holder = held.unwrap_or_else(|| panic!("built"));
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert_eq!(function.sized_stack_slots[StackSlot::from_u32(0)].size, 8);
    assert_eq!(function.sized_stack_slots[holder].size, 0);
}

#[test]
fn a_function_reading_the_frame_or_stack_pointer_or_switching_stacks_is_left_alone() {
    type Reach = fn(&mut FunctionBuilder<'_>);
    let reaches: [(&str, Reach); 3] = [
        ("get_frame_pointer", |builder| {
            builder.ins().get_frame_pointer(types::I64);
        }),
        ("get_stack_pointer", |builder| {
            builder.ins().get_stack_pointer(types::I64);
        }),
        ("stack_switch", |builder| {
            let zero = builder.ins().iconst(types::I64, 0);
            builder.ins().stack_switch(zero, zero, zero);
        }),
    ];
    for (name, reach) in reaches {
        let mut function = stored_through(|builder, value, _| {
            reach(builder);
            builder.ins().stack_addr(types::I64, value, 0)
        });
        let isa = host_isa();
        let promotion = promote(&mut Context::new(), &mut function, &*isa)
            .unwrap_or_else(|e| panic!("promotion succeeds: {e}"));
        assert_eq!(promotion, Promotion::default(), "{name}:\n{function}");
    }
}

#[test]
fn a_cell_past_the_start_of_its_slot_is_promoted_alone() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 16);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, value, 8);
        let read = builder.ins().stack_load(types::I64, value, 8);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion { slots: 1, cells: 1 });
    assert_eq!(slot_sizes(&function), vec![0], "{function}");
}

#[test]
fn a_cell_of_a_wide_integer_or_a_vector_is_promoted() {
    for (ty, size) in [(types::I128, 16), (types::I32X4, 16), (types::I8, 1)] {
        let mut function = function(&[ty], &[ty], |builder, entry| {
            let value = slot(builder, size);
            let argument = param(builder, entry, 0);
            builder.ins().stack_store(argument, value, 0);
            let read = builder.ins().stack_load(ty, value, 0);
            builder.ins().return_(&[read]);
        });
        assert_eq!(
            promoted(&mut function),
            Promotion { slots: 1, cells: 1 },
            "{ty}"
        );
    }
}

#[test]
fn a_cell_of_a_half_or_quad_precision_float_is_kept() {
    for (ty, size) in [(types::F16, 2), (types::F128, 16)] {
        let mut function = function(&[ty], &[ty], |builder, entry| {
            let value = slot(builder, size);
            let argument = param(builder, entry, 0);
            builder.ins().stack_store(argument, value, 0);
            let read = builder.ins().stack_load(ty, value, 0);
            builder.ins().return_(&[read]);
        });
        let isa = host_isa();
        let promotion = promote(&mut Context::new(), &mut function, &*isa)
            .unwrap_or_else(|e| panic!("promotion succeeds: {e}"));
        assert_eq!(promotion, Promotion::default(), "{ty}:\n{function}");
    }
}

#[test]
fn a_promoted_function_uses_no_alias() {
    let mut function = three_field_value();
    promoted(&mut function);
    for block in function.layout.blocks() {
        for inst in function.layout.block_insts(block) {
            for value in function.dfg.inst_values(inst) {
                assert!(
                    function.dfg.value_is_real(value),
                    "{inst} uses alias {value}:\n{function}"
                );
            }
        }
    }
}

/// A function storing `cells` 8-byte values into one slot and reading each
/// back, through a chain of `blocks` blocks, beside a one-cell slot.
fn many_cells_over_many_blocks(cells: u32, blocks: usize) -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let small = slot(builder, 8);
        let large = slot(builder, cells * 8);
        let argument = param(builder, entry, 0);
        builder.ins().stack_store(argument, small, 0);
        for cell in 0..cells {
            let offset = i32::try_from(cell * 8).unwrap_or_else(|e| panic!("{e}"));
            builder.ins().stack_store(argument, large, offset);
        }
        for _ in 0..blocks {
            let next = builder.create_block();
            builder.ins().jump(next, &[]);
            builder.switch_to_block(next);
        }
        let mut total = builder.ins().stack_load(types::I64, small, 0);
        for cell in 0..cells {
            let offset = i32::try_from(cell * 8).unwrap_or_else(|e| panic!("{e}"));
            let read = builder.ins().stack_load(types::I64, large, offset);
            total = builder.ins().bxor(total, read);
        }
        builder.ins().return_(&[total]);
    })
}

#[test]
fn a_slot_whose_cells_over_the_blocks_cost_too_much_is_kept() {
    let blocks = 600;
    let cells = u32::try_from(MAX_CELL_BLOCKS / blocks).unwrap_or_else(|e| panic!("{e}"));
    let mut over = many_cells_over_many_blocks(cells, blocks);
    assert_eq!(promoted(&mut over), Promotion { slots: 1, cells: 1 });
    assert_eq!(slot_sizes(&over), vec![0, cells * 8]);
    let mut under = many_cells_over_many_blocks(cells / 2, blocks);
    assert_eq!(promoted(&mut under).slots, 2);
}

/// `fn(i64) i64` counting up from its argument to 10 in a slot whose
/// address the loop header takes as a parameter, as the frontend leaves a
/// variable holding the address, the inner loop of `nested` taking it
/// again from the outer one.
fn address_through_headers(nested: bool) -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let counter = slot(builder, 8);
        let (top, body, exit) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let address = builder.ins().stack_addr(types::I64, counter, 0);
        let start = param(builder, entry, 0);
        builder.ins().store(MemFlags::new(), start, address, 0);
        builder.ins().jump(top, &[address.into()]);
        let carried = builder.append_block_param(top, types::I64);
        builder.switch_to_block(top);
        let count = builder.ins().load(types::I64, MemFlags::new(), carried, 0);
        let ten = builder.ins().iconst(types::I64, 10);
        let more = builder.ins().icmp(IntCC::SignedLessThan, count, ten);
        builder.ins().brif(more, body, &[], exit, &[]);
        builder.switch_to_block(body);
        let mut latch_address = carried;
        if nested {
            let inner = builder.create_block();
            let again = builder.append_block_param(inner, types::I64);
            builder.ins().jump(inner, &[carried.into()]);
            builder.switch_to_block(inner);
            latch_address = again;
        }
        let next = builder.ins().iadd_imm(count, 1);
        builder.ins().store(MemFlags::new(), next, latch_address, 0);
        builder.ins().jump(top, &[latch_address.into()]);
        builder.switch_to_block(exit);
        let last = builder.ins().load(types::I64, MemFlags::new(), carried, 0);
        builder.ins().return_(&[last]);
    })
}

#[test]
fn an_address_a_loop_header_takes_as_a_parameter_is_still_the_slots() {
    for nested in [false, true] {
        let mut function = address_through_headers(nested);
        assert_eq!(
            promoted(&mut function),
            Promotion { slots: 1, cells: 1 },
            "{function}"
        );
        assert!(!touches_memory(&function), "{function}");
        assert_eq!(slot_sizes(&function), vec![0], "{function}");
    }
}

#[test]
fn a_parameter_joining_the_addresses_of_two_slots_keeps_both() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let (first, second) = (slot(builder, 8), slot(builder, 8));
        let join = builder.create_block();
        let joined = builder.append_block_param(join, types::I64);
        let argument = param(builder, entry, 0);
        let mut addresses = Vec::new();
        for value in [first, second] {
            let address = builder.ins().stack_addr(types::I64, value, 0);
            builder.ins().store(MemFlags::new(), argument, address, 0);
            addresses.push(address);
        }
        builder.ins().brif(
            argument,
            join,
            &[addresses[0].into()],
            join,
            &[addresses[1].into()],
        );
        builder.switch_to_block(join);
        let read = builder.ins().load(types::I64, MemFlags::new(), joined, 0);
        builder.ins().return_(&[read]);
    });
    assert_eq!(promoted(&mut function), Promotion::default(), "{function}");
    assert_eq!(slot_sizes(&function), vec![8, 8], "{function}");
}

#[test]
fn an_address_handed_through_a_shared_jump_table_is_forwarded_once() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let (left, right, join) = (
            builder.create_block(),
            builder.create_block(),
            builder.create_block(),
        );
        let carried = builder.append_block_param(join, types::I64);
        let tag = builder.append_block_param(join, types::I64);
        let argument = param(builder, entry, 0);
        let address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        let seven = builder.ins().iconst(types::I64, 7);
        let selector = builder.ins().ireduce(types::I32, argument);
        builder.ins().brif(argument, left, &[], right, &[]);
        let handed = [address.into(), seven.into()];
        let default = builder.func.dfg.block_call(join, &handed);
        let entries = [builder.func.dfg.block_call(join, &handed)];
        let table = builder.create_jump_table(JumpTableData::new(default, &entries));
        for arm in [left, right] {
            builder.switch_to_block(arm);
            builder.ins().br_table(selector, table);
        }
        builder.switch_to_block(join);
        let read = builder.ins().load(types::I64, MemFlags::new(), carried, 0);
        let sum = builder.ins().iadd(read, tag);
        builder.ins().return_(&[sum]);
    });
    assert_eq!(
        promoted(&mut function),
        Promotion { slots: 1, cells: 1 },
        "{function}"
    );
    assert!(!touches_memory(&function), "{function}");
}

#[test]
fn an_address_derived_from_a_forwarded_parameter_is_forwarded_too() {
    let mut function = function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 16);
        let blocks: Vec<Block> = (0..6).map(|_| builder.create_block()).collect();
        let (outer, inner_start, inner, latch, outer_latch, exit) = (
            blocks[0], blocks[1], blocks[2], blocks[3], blocks[4], blocks[5],
        );
        let base = builder.ins().stack_addr(types::I64, value, 0);
        let start = param(builder, entry, 0);
        builder.ins().store(MemFlags::new(), start, base, 8);
        builder.ins().jump(outer, &[base.into()]);
        let carried = builder.append_block_param(outer, types::I64);
        builder.switch_to_block(outer);
        let total = builder.ins().load(types::I64, MemFlags::new(), carried, 8);
        let limit = builder.ins().iconst(types::I64, 20);
        let more = builder.ins().icmp(IntCC::SignedLessThan, total, limit);
        builder.ins().brif(more, inner_start, &[], exit, &[]);
        builder.switch_to_block(inner_start);
        let field = builder.ins().iadd_imm(carried, 8);
        builder.ins().jump(inner, &[field.into()]);
        let inner_field = builder.append_block_param(inner, types::I64);
        builder.switch_to_block(inner);
        let count = builder
            .ins()
            .load(types::I64, MemFlags::new(), inner_field, 0);
        let ten = builder.ins().iconst(types::I64, 10);
        let again = builder.ins().icmp(IntCC::SignedLessThan, count, ten);
        builder.ins().brif(again, latch, &[], outer_latch, &[]);
        builder.switch_to_block(latch);
        let next = builder.ins().iadd_imm(count, 1);
        builder.ins().store(MemFlags::new(), next, inner_field, 0);
        builder.ins().jump(inner, &[inner_field.into()]);
        builder.switch_to_block(outer_latch);
        builder.ins().jump(outer, &[carried.into()]);
        builder.switch_to_block(exit);
        builder.ins().return_(&[total]);
    });
    assert_eq!(
        promoted(&mut function),
        Promotion { slots: 1, cells: 1 },
        "{function}"
    );
    assert!(!touches_memory(&function), "{function}");
}

/// A function handing a slot's address down a chain of `depth` blocks,
/// each adding nothing to the address its parameter takes and handing the
/// sum on, the last reading through it.
fn address_down_a_chain(depth: usize) -> Function {
    function(&[types::I64], &[types::I64], |builder, entry| {
        let value = slot(builder, 8);
        let argument = param(builder, entry, 0);
        let mut address = builder.ins().stack_addr(types::I64, value, 0);
        builder.ins().store(MemFlags::new(), argument, address, 0);
        for _ in 0..depth {
            let next = builder.create_block();
            let parameter = builder.append_block_param(next, types::I64);
            builder.ins().jump(next, &[address.into()]);
            builder.switch_to_block(next);
            address = builder.ins().iadd_imm(parameter, 0);
        }
        let read = builder.ins().load(types::I64, MemFlags::new(), address, 0);
        builder.ins().return_(&[read]);
    })
}

#[test]
fn forwarding_stops_after_its_last_round_however_deep_the_chain() {
    let mut shallow = address_down_a_chain(MAX_FORWARDING_ROUNDS / 2);
    assert_eq!(promoted(&mut shallow), Promotion { slots: 1, cells: 1 });
    let mut deep = address_down_a_chain(MAX_FORWARDING_ROUNDS * 2);
    assert_eq!(promoted(&mut deep), Promotion::default());
    assert_eq!(slot_sizes(&deep), vec![8]);
}
