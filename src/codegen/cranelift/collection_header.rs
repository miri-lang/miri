// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reads of the header an array or a list keeps its elements behind.
//!
//! `MiriArray { data, elem_count, elem_size, .. }` and `MiriList { data, len,
//! capacity, elem_size, .. }` keep the data pointer in their first word and the
//! element count in their second; the slot size follows at a different word in
//! each. Every word of a header that code generation reads goes through here,
//! so an index, a length and the slot-size check agree on where it lives.
//!
//! None of these loads is marked read-only or movable. A push writes the
//! length through the same pointer, and a load allowed to move could be
//! hoisted above the null check that guards it.

use crate::ast::types::{BuiltinCollectionKind, Type};
use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::error::CodegenError;

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{InstBuilder, MemFlags, TrapCode, Value};
use cranelift_frontend::FunctionBuilder;

/// User trap code raised when a collection that stores its elements inline was
/// allocated with slots of a size other than the stride those elements are
/// addressed at.
///
/// Built via `TrapCode::unwrap_user`, a `const fn`, so an invalid code fails to
/// compile rather than panic at run time.
const INLINE_STRIDE_TRAP_CODE: TrapCode = TrapCode::unwrap_user(3);

/// Word offsets of the fields of an array's or a list's header, laid out in C
/// order: the data pointer and the element count are shared, the rest is
/// each kind's own.
pub(crate) const HEADER_DATA: i32 = 0;
pub(crate) const HEADER_LEN: i32 = 1;
pub(crate) const ARRAY_SLOT_SIZE: i32 = 2;
pub(crate) const LIST_CAPACITY: i32 = 2;
pub(crate) const LIST_SLOT_SIZE: i32 = 3;

/// The words of an array's or a list's header an element access reads.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ElementHeader {
    /// The address of the first element.
    pub(crate) data: Value,
    /// The number of elements.
    pub(crate) len: Value,
}

impl FunctionTranslator<'_> {
    /// Load the word at `offset` in the header at `base`.
    pub(crate) fn load_header_word(
        builder: &mut FunctionBuilder,
        base: Value,
        offset: i32,
        ptr_type: cl_types::Type,
    ) -> Value {
        builder.ins().load(ptr_type, MemFlags::new(), base, offset)
    }

    /// Load the data pointer and the element count of the array or list at
    /// `base`, first trapping unless its slots are `stride` bytes wide when
    /// its elements are addressed at a stride of their own.
    pub(crate) fn load_element_header(
        builder: &mut FunctionBuilder,
        base: Value,
        base_type: &Type,
        stride: Option<i64>,
        ptr_type: cl_types::Type,
    ) -> ElementHeader {
        if let Some(stride) = stride {
            Self::trap_unless_slots_match_stride(builder, base, base_type, stride, ptr_type);
        }
        let word = ptr_type.bytes() as i32;
        ElementHeader {
            data: Self::load_header_word(builder, base, HEADER_DATA * word, ptr_type),
            len: Self::load_header_word(builder, base, HEADER_LEN * word, ptr_type),
        }
    }

    /// Trap unless the collection at `base` was allocated with slots of exactly
    /// `stride` bytes.
    ///
    /// An inline element is addressed at its stride. A collection whose slots are
    /// any other size would be read and written at the wrong offsets — handing
    /// back bytes that belong to a neighbouring element, or lie past the end of
    /// the buffer — so the program stops instead of reading them.
    ///
    /// The trap sits in a cold block of its own rather than behind a
    /// conditional trap: Cranelift's alias analysis treats a conditional trap
    /// as a fence every load must stay behind, which would keep each header
    /// load after it pinned in place.
    pub(crate) fn trap_unless_slots_match_stride(
        builder: &mut FunctionBuilder,
        base: Value,
        base_type: &Type,
        stride: i64,
        ptr_type: cl_types::Type,
    ) {
        let Some(offset) = Self::slot_size_offset(base_type, ptr_type.bytes() as i32) else {
            return;
        };
        let slot_size = Self::load_header_word(builder, base, offset, ptr_type);
        let mismatch = builder.ins().icmp_imm(IntCC::NotEqual, slot_size, stride);
        Self::trap_cold_if(builder, mismatch);
    }

    /// Branch to a cold path raising the stride trap when `condition` holds,
    /// and continue in a fresh block otherwise.
    ///
    /// The branch goes to a block that jumps to the trap rather than to the
    /// trap itself: Cranelift's legalizer rewrites a branch to a block holding
    /// nothing but a trap into the conditional trap this avoids.
    fn trap_cold_if(builder: &mut FunctionBuilder, condition: Value) {
        let cold_block = builder.create_block();
        let trap_block = builder.create_block();
        let continue_block = builder.create_block();
        builder.set_cold_block(cold_block);
        builder.set_cold_block(trap_block);
        builder
            .ins()
            .brif(condition, cold_block, &[], continue_block, &[]);
        builder.switch_to_block(cold_block);
        builder.seal_block(cold_block);
        builder.ins().jump(trap_block, &[]);
        builder.switch_to_block(trap_block);
        builder.seal_block(trap_block);
        builder.ins().trap(INLINE_STRIDE_TRAP_CODE);
        builder.switch_to_block(continue_block);
        builder.seal_block(continue_block);
    }

    /// Offset of the slot-size field in the runtime header of an array or list:
    /// `MiriArray { data, elem_count, elem_size, .. }` and
    /// `MiriList { data, len, capacity, elem_size, .. }`. Neither a map nor a set
    /// is addressed by index, so neither has a slot size to check.
    fn slot_size_offset(base_type: &Type, ptr_size: i32) -> Option<i32> {
        match base_type.kind.as_builtin_collection()? {
            BuiltinCollectionKind::Array => Some(ARRAY_SLOT_SIZE * ptr_size),
            BuiltinCollectionKind::List => Some(LIST_SLOT_SIZE * ptr_size),
            BuiltinCollectionKind::Map | BuiltinCollectionKind::Set => None,
        }
    }
}

/// Where an index finds the collection it reads or writes.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CollectionBase {
    /// The collection's address, its header read at the access.
    Address(Value),
    /// The data pointer and length of a collection whose header was read
    /// before the loop the access is in.
    Hoisted(ElementHeader),
    /// A hoisted header, with the index proven below its length and
    /// non-negative (`Body::in_bounds`), so the access needs no bounds check.
    Proven(ElementHeader),
}

impl CollectionBase {
    /// The collection's address, which only a header read at the access has.
    pub(crate) fn address(self, base_type: &Type) -> Result<Value, CodegenError> {
        match self {
            CollectionBase::Address(address) => Ok(address),
            CollectionBase::Hoisted(_) | CollectionBase::Proven(_) => {
                Err(CodegenError::Internal(format!(
                    "a `{}` is indexed through a header read before the loop, but it keeps none",
                    base_type.kind
                )))
            }
        }
    }

    /// Whether an access through this base compares its index to the length.
    pub(crate) fn needs_bounds_check(self) -> bool {
        match self {
            CollectionBase::Address(_) | CollectionBase::Hoisted(_) => true,
            CollectionBase::Proven(_) => false,
        }
    }
}
