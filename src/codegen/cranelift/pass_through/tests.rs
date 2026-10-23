// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Forwarding of pass-through block parameters over hand-built CLIF.

use super::*;
use cranelift_codegen::ir::{
    types, AbiParam, BlockCall, InstBuilder, JumpTableData, Signature, UserFuncName,
};
use cranelift_codegen::isa::{CallConv, TargetFrontendConfig};
use cranelift_codegen::settings;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// `fn(i64, i64) i64` built by `body` from its entry block on.
fn function(body: impl FnOnce(&mut FunctionBuilder<'_>, Block)) -> Function {
    let mut signature = Signature::new(CallConv::SystemV);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    let mut function = Function::with_name_signature(UserFuncName::default(), signature);
    let mut context = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut function, &mut context);
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    body(&mut builder, entry);
    builder.seal_all_blocks();
    builder.finalize(TargetFrontendConfig {
        default_call_conv: CallConv::SystemV,
        pointer_width: target_lexicon::PointerWidth::U64,
        page_size_align_log2: 12,
    });
    function
}

/// Forward `function`'s pass-through parameters, checking it verifies after.
fn forwarded(function: &mut Function) {
    forward_pass_through_parameters(function);
    let flags = settings::Flags::new(settings::builder());
    if let Err(errors) = cranelift_codegen::verify_function(function, &flags) {
        panic!("the function does not verify: {errors}\n{function}");
    }
}

/// The `index`-th block of `function` in layout order.
fn block(function: &Function, index: usize) -> Block {
    (function.layout.blocks())
        .nth(index)
        .unwrap_or_else(|| panic!("no block {index} in:\n{function}"))
}

/// How many arguments each branch from `from` to `to` hands it.
fn arguments_handed(function: &Function, from: Block, to: Block) -> Vec<usize> {
    let dfg = &function.dfg;
    let Some(branch) = function.layout.last_inst(from) else {
        return Vec::new();
    };
    (dfg.insts[branch]
        .branch_destination(&dfg.jump_tables, &dfg.exception_tables)
        .iter())
    .filter(|call| call.block(&dfg.value_lists) == to)
    .map(|call| call.args(&dfg.value_lists).count())
    .collect()
}

/// Whether some instruction of `block` reads `value`.
fn reads(function: &Function, block: Block, value: Value) -> bool {
    (function.layout.block_insts(block))
        .any(|inst| function.dfg.inst_values(inst).any(|read| read == value))
}

/// `fn(i64, i64) i64` looping on a header with two parameters: one every
/// back edge hands back unchanged, and a counter it increments.
fn loop_with_a_pass_through_and_a_counter() -> Function {
    function(|builder, entry| {
        let header = builder.create_block();
        let exit = builder.create_block();
        let carried = builder.append_block_param(header, types::I64);
        let counter = builder.append_block_param(header, types::I64);
        let argument = builder.block_params(entry)[0];
        let zero = builder.ins().iconst(types::I64, 0);
        builder.ins().jump(header, &[argument.into(), zero.into()]);
        builder.switch_to_block(header);
        let next = builder.ins().iadd(counter, carried);
        builder
            .ins()
            .brif(next, header, &[carried.into(), next.into()], exit, &[]);
        builder.switch_to_block(exit);
        builder.ins().return_(&[next]);
    })
}

#[test]
fn a_parameter_every_back_edge_hands_back_is_replaced_and_a_counter_kept() {
    let mut function = loop_with_a_pass_through_and_a_counter();
    let argument = function.dfg.block_params(block(&function, 0))[0];
    let header = block(&function, 1);
    forwarded(&mut function);
    assert_eq!(function.dfg.num_block_params(header), 1, "{function}");
    assert_ne!(function.dfg.block_params(header)[0], argument, "{function}");
    assert!(
        reads(&function, header, argument),
        "the header no longer reads the argument:\n{function}"
    );
    assert_eq!(
        arguments_handed(&function, header, header),
        vec![1],
        "{function}"
    );
    assert_eq!(
        arguments_handed(&function, block(&function, 0), header),
        vec![1],
        "{function}"
    );
}

#[test]
fn a_parameter_a_back_edge_hands_another_value_is_kept() {
    let mut function = function(|builder, entry| {
        let header = builder.create_block();
        let exit = builder.create_block();
        let counter = builder.append_block_param(header, types::I64);
        let argument = builder.block_params(entry)[0];
        builder.ins().jump(header, &[argument.into()]);
        builder.switch_to_block(header);
        let next = builder.ins().iadd_imm_s(counter, 1);
        builder.ins().brif(next, header, &[next.into()], exit, &[]);
        builder.switch_to_block(exit);
        builder.ins().return_(&[counter]);
    });
    let header = block(&function, 1);
    forwarded(&mut function);
    assert_eq!(function.dfg.num_block_params(header), 1, "{function}");
}

#[test]
fn a_parameter_handed_only_through_a_later_block_is_replaced_in_a_second_round() {
    // The header's back edge comes from a latch laid out after it whose own
    // parameter is the header's: the header sees two values until the
    // latch's parameter is replaced.
    let mut function = function(|builder, entry| {
        let header = builder.create_block();
        let latch = builder.create_block();
        let exit = builder.create_block();
        let carried = builder.append_block_param(header, types::I64);
        let relayed = builder.append_block_param(latch, types::I64);
        let argument = builder.block_params(entry)[0];
        let condition = builder.block_params(entry)[1];
        builder.ins().jump(header, &[argument.into()]);
        builder.switch_to_block(header);
        builder.ins().jump(latch, &[carried.into()]);
        builder.switch_to_block(latch);
        builder
            .ins()
            .brif(condition, header, &[relayed.into()], exit, &[]);
        builder.switch_to_block(exit);
        builder.ins().return_(&[relayed]);
    });
    let argument = function.dfg.block_params(block(&function, 0))[0];
    forwarded(&mut function);
    for index in [1, 2] {
        let block = block(&function, index);
        assert_eq!(function.dfg.num_block_params(block), 0, "{function}");
    }
    assert!(
        reads(&function, block(&function, 3), argument),
        "{function}"
    );
}

#[test]
fn a_parameter_handed_through_a_jump_table_two_branches_share_is_replaced() {
    let mut function = function(|builder, entry| {
        let left = builder.create_block();
        let right = builder.create_block();
        let target = builder.create_block();
        let parameter = builder.append_block_param(target, types::I64);
        let argument = builder.block_params(entry)[0];
        let condition = builder.block_params(entry)[1];
        let index = builder.ins().ireduce(types::I32, condition);
        let call = |builder: &mut FunctionBuilder<'_>| {
            BlockCall::new(target, [argument.into()], &mut builder.func.dfg.value_lists)
        };
        let default = call(builder);
        let entries = vec![call(builder), call(builder)];
        let table = builder.create_jump_table(JumpTableData::new(default, &entries));
        builder.ins().brif(condition, left, &[], right, &[]);
        builder.switch_to_block(left);
        builder.ins().br_table(index, table);
        builder.switch_to_block(right);
        builder.ins().br_table(index, table);
        builder.switch_to_block(target);
        builder.ins().return_(&[parameter]);
    });
    let argument = function.dfg.block_params(block(&function, 0))[0];
    let target = block(&function, 3);
    forwarded(&mut function);
    assert_eq!(function.dfg.num_block_params(target), 0, "{function}");
    assert_eq!(
        arguments_handed(&function, block(&function, 1), target),
        vec![0, 0, 0],
        "{function}"
    );
    assert!(reads(&function, target, argument), "{function}");
}

#[test]
fn a_parameter_both_targets_of_one_branch_hand_the_same_value_is_replaced() {
    let mut function = function(|builder, entry| {
        let target = builder.create_block();
        let parameter = builder.append_block_param(target, types::I64);
        let argument = builder.block_params(entry)[0];
        let condition = builder.block_params(entry)[1];
        builder.ins().brif(
            condition,
            target,
            &[argument.into()],
            target,
            &[argument.into()],
        );
        builder.switch_to_block(target);
        builder.ins().return_(&[parameter]);
    });
    let target = block(&function, 1);
    forwarded(&mut function);
    assert_eq!(function.dfg.num_block_params(target), 0, "{function}");
    assert_eq!(
        arguments_handed(&function, block(&function, 0), target),
        vec![0, 0],
        "{function}"
    );
}

#[test]
fn a_parameter_an_unreachable_block_hands_another_value_is_kept() {
    let mut function = function(|builder, entry| {
        let target = builder.create_block();
        let unreachable = builder.create_block();
        let parameter = builder.append_block_param(target, types::I64);
        let argument = builder.block_params(entry)[0];
        let other = builder.block_params(entry)[1];
        builder.ins().jump(target, &[argument.into()]);
        builder.switch_to_block(target);
        builder.ins().return_(&[parameter]);
        builder.switch_to_block(unreachable);
        builder.ins().jump(target, &[other.into()]);
    });
    let target = block(&function, 1);
    forwarded(&mut function);
    assert_eq!(function.dfg.num_block_params(target), 1, "{function}");
}

#[test]
fn two_parameters_that_hand_each_other_back_are_kept() {
    // Both are the argument on every path, but each sees the other along
    // the back edge; the pass keeps them rather than reason about cycles.
    let mut function = function(|builder, entry| {
        let header = builder.create_block();
        let exit = builder.create_block();
        let first = builder.append_block_param(header, types::I64);
        let second = builder.append_block_param(header, types::I64);
        let argument = builder.block_params(entry)[0];
        let condition = builder.block_params(entry)[1];
        builder
            .ins()
            .jump(header, &[argument.into(), argument.into()]);
        builder.switch_to_block(header);
        builder
            .ins()
            .brif(condition, header, &[second.into(), first.into()], exit, &[]);
        builder.switch_to_block(exit);
        let sum = builder.ins().iadd(first, second);
        builder.ins().return_(&[sum]);
    });
    let header = block(&function, 1);
    forwarded(&mut function);
    assert_eq!(function.dfg.num_block_params(header), 2, "{function}");
}

#[test]
fn a_function_with_no_block_parameters_is_left_as_it_is() {
    let mut function = function(|builder, entry| {
        let argument = builder.block_params(entry)[0];
        builder.ins().return_(&[argument]);
    });
    let before = function.display().to_string();
    forwarded(&mut function);
    assert_eq!(function.display().to_string(), before);
}
