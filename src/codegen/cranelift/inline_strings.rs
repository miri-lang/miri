// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! String operations compiled inline for their common case, with the runtime
//! call kept for every other case. They read and write the runtime's
//! `MiriString` header at the offsets below.

use crate::codegen::cranelift::translator::{FunctionTranslator, ModuleCtx, RuntimeChecks};
use crate::error::CodegenError;
use crate::mir::symbol::Symbol;
use crate::runtime_fns::rt;
use cranelift_codegen::ir::{
    condcodes::IntCC, types, AbiParam, InstBuilder, MemFlagsData, Signature, Value,
};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{Linkage, Module};

/// Where a `MiriString` header keeps its bytes, its byte length and its
/// capacity, in pointer-sized words from its start, and the bytes past the
/// third word that hold its ASCII flag and its storage kind. The runtime's
/// `test_string_header_sits_where_compiled_code_reads_and_writes_it` holds the
/// header to these.
pub(super) const STRING_DATA_WORD: i32 = 0;
pub(super) const STRING_LEN_WORD: i32 = 1;
const STRING_CAPACITY_WORD: i32 = 2;
pub(super) const STRING_ASCII_WORD: i32 = 3;
const STRING_STORAGE_BYTE: i32 = 1;
/// The storage kind of bytes a string does not own, such as a literal's.
const STORAGE_UNOWNED: i64 = 0;

/// The two strings of an append, `left += right`.
#[derive(Clone, Copy)]
pub(super) struct Append {
    pub(super) left: Value,
    pub(super) right: Value,
}

impl<'a> FunctionTranslator<'a> {
    /// Answers `miri_rt_string_append(left, right)` in place when the runtime
    /// would append in place and nothing more: `left` is held only here, owns
    /// its bytes, is marked ASCII, and has room for `right`'s bytes, and the
    /// two are different strings. The piece's bytes are copied after `left`'s,
    /// its length grows, and its ASCII mark becomes `right`'s, as the runtime
    /// sets them. A left side marked non-ASCII takes the call, because the
    /// runtime also forgets what it remembered about that string's characters.
    /// Emitted only in a build with the runtime's checks compiled out, so a
    /// build that keeps them still has the heap guard see both strings.
    pub(super) fn emit_inline_string_append(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        append: Append,
        sig: &Signature,
    ) -> Result<Value, CodegenError> {
        let ptr_type = builder.func.dfg.value_type(append.left);
        let header_block = builder.create_block();
        let copy_block = builder.create_block();
        let runtime_block = builder.create_block();
        let join_block = builder.create_block();
        builder.append_block_param(join_block, ptr_type);

        let both = Self::emit_both_non_null(builder, append);
        builder
            .ins()
            .brif(both, header_block, &[], runtime_block, &[]);

        builder.switch_to_block(header_block);
        builder.seal_block(header_block);
        let room = Self::emit_append_room(builder, append, ptr_type);
        builder
            .ins()
            .brif(room.fits, copy_block, &[], runtime_block, &[]);

        builder.switch_to_block(copy_block);
        builder.seal_block(copy_block);
        Self::emit_append_copy(builder, ctx, append, &room, ptr_type)?;
        builder.ins().jump(join_block, &[append.left.into()]);

        builder.switch_to_block(runtime_block);
        builder.seal_block(runtime_block);
        let answer = Self::call_append_runtime(builder, ctx, callee, append, sig)?;
        builder.ins().jump(join_block, &[answer.into()]);

        builder.seal_block(join_block);
        builder.switch_to_block(join_block);
        Ok(builder.block_params(join_block)[0])
    }

    /// Whether neither string is null.
    fn emit_both_non_null(builder: &mut FunctionBuilder, append: Append) -> Value {
        let left = builder.ins().icmp_imm_u(IntCC::NotEqual, append.left, 0);
        let right = builder.ins().icmp_imm_u(IntCC::NotEqual, append.right, 0);
        builder.ins().band(left, right)
    }

    /// Reads both headers and decides whether the append can be made in
    /// place, keeping what the copy needs.
    fn emit_append_room(
        builder: &mut FunctionBuilder,
        append: Append,
        ptr_type: types::Type,
    ) -> AppendRoom {
        let word = ptr_type.bytes() as i32;
        let header = MemFlagsData::trusted();
        let count = builder.ins().load(ptr_type, header, append.left, -word);
        let unique = builder.ins().icmp_imm_s(IntCC::Equal, count, 1);
        let distinct = builder
            .ins()
            .icmp(IntCC::NotEqual, append.left, append.right);
        let flags_at = word * STRING_ASCII_WORD;
        let ascii = builder
            .ins()
            .uload8(types::I32, header, append.left, flags_at);
        let is_ascii = builder.ins().icmp_imm_u(IntCC::NotEqual, ascii, 0);
        let storage = builder.ins().uload8(
            types::I32,
            header,
            append.left,
            flags_at + STRING_STORAGE_BYTE,
        );
        let owned = builder
            .ins()
            .icmp_imm_u(IntCC::NotEqual, storage, STORAGE_UNOWNED);
        let len = builder
            .ins()
            .load(ptr_type, header, append.left, word * STRING_LEN_WORD);
        let piece_len = builder
            .ins()
            .load(ptr_type, header, append.right, word * STRING_LEN_WORD);
        let capacity =
            builder
                .ins()
                .load(ptr_type, header, append.left, word * STRING_CAPACITY_WORD);
        // Both lengths are below isize::MAX, so their sum cannot wrap.
        let total = builder.ins().iadd(len, piece_len);
        let has_room = builder
            .ins()
            .icmp(IntCC::UnsignedLessThanOrEqual, total, capacity);
        let held_alone = builder.ins().band(unique, distinct);
        let writable = builder.ins().band(is_ascii, owned);
        let fits = builder.ins().band(held_alone, writable);
        let fits = builder.ins().band(fits, has_room);
        AppendRoom {
            fits,
            len,
            piece_len,
            total,
        }
    }

    /// Copies the piece's bytes after `left`'s and updates `left`'s length and
    /// ASCII mark.
    fn emit_append_copy(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        append: Append,
        room: &AppendRoom,
        ptr_type: types::Type,
    ) -> Result<(), CodegenError> {
        let word = ptr_type.bytes() as i32;
        let header = MemFlagsData::trusted();
        let data = builder
            .ins()
            .load(ptr_type, header, append.left, word * STRING_DATA_WORD);
        let dest = builder.ins().iadd(data, room.len);
        let source = builder
            .ins()
            .load(ptr_type, header, append.right, word * STRING_DATA_WORD);
        let copy = Self::import_copy_bytes(builder, ctx, ptr_type)?;
        builder.ins().call(copy, &[dest, source, room.piece_len]);
        builder
            .ins()
            .store(header, room.total, append.left, word * STRING_LEN_WORD);
        let flags_at = word * STRING_ASCII_WORD;
        let piece_ascii = builder
            .ins()
            .load(types::I8, header, append.right, flags_at);
        builder
            .ins()
            .store(header, piece_ascii, append.left, flags_at);
        Ok(())
    }

    /// The runtime's `miri_rt_string_copy_bytes(dest, source, size)`, declared
    /// in this function.
    fn import_copy_bytes(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: types::Type,
    ) -> Result<cranelift_codegen::ir::FuncRef, CodegenError> {
        let mut sig = Signature::new(builder.func.signature.call_conv);
        sig.params.extend([AbiParam::new(ptr_type); 3]);
        let func_id = ctx
            .module
            .declare_function(rt::STRING_COPY_BYTES, Linkage::Import, &sig)
            .map_err(|e| {
                CodegenError::declare_function(rt::STRING_COPY_BYTES.to_string(), e.to_string())
            })?;
        Ok(ctx.module.declare_func_in_func(func_id, builder.func))
    }

    /// Calls `miri_rt_string_append` and returns the string it hands back.
    fn call_append_runtime(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        callee: &Symbol,
        append: Append,
        sig: &Signature,
    ) -> Result<Value, CodegenError> {
        let func_name = callee.link_name();
        let func_id = ctx
            .module
            .declare_function(&func_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(func_name, e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        let call = builder.ins().call(local_func, &[append.left, append.right]);
        builder
            .inst_results(call)
            .first()
            .copied()
            .ok_or_else(|| CodegenError::Internal(format!("{} returns nothing", rt::STRING_APPEND)))
    }

    /// Whether a call to `callee` is an append this build compiles inline.
    pub(super) fn is_inline_string_append(ctx: &ModuleCtx, callee: &Symbol) -> bool {
        callee.runtime_name() == Some(rt::STRING_APPEND)
            && ctx.runtime_checks == RuntimeChecks::CompiledOut
    }
}

/// What the header reads of an append decided: whether it fits in place, and
/// the lengths the copy uses.
struct AppendRoom {
    fits: Value,
    len: Value,
    piece_len: Value,
    total: Value,
}
