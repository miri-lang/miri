// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The collection headers a function reads once per loop.
//!
//! MIR marks, with a `ReadHeader` statement at the end of a loop's
//! preheader, each array or list the loop leaves the header of unchanged.
//! There the header is read once:
//! null-guarded, so a collection never allocated reads as no data and no
//! elements, as `Len` reads it; and, for elements held at a stride of their
//! own, with the slot-size check run only when there is an element to check.
//! An element access or a length inside the loop rooted at the collection
//! then reads the two words from the variables those loads defined.
//!
//! Where a collection is read over nested loops, the outermost read covers
//! the blocks they share, so the header is read once for the whole nest. MIR
//! guarantees one read per block and collection and that each preheader
//! dominates its blocks: code generation looks the read up and decides
//! nothing about the control flow.

use crate::mir::header_reads::stable_collections;
use std::cell::Cell;
use std::collections::HashMap;

use crate::codegen::cranelift::collection_header::ElementHeader;
use crate::codegen::cranelift::translator::{inline_stride, FunctionTranslator, TypeCtx};
use crate::error::CodegenError;
use crate::mir::{Body, Local, Place};

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_frontend::{FunctionBuilder, Variable};

/// The variables one record's header is read into.
#[derive(Debug, Clone, Copy)]
struct HoistedHeader {
    local: Local,
    preheader: usize,
    data: Variable,
    len: Variable,
}

/// The headers the function being translated reads once per loop, and the
/// MIR block being translated, which decides whether an access may use them.
#[derive(Debug, Default)]
pub struct StableHeaders {
    hoisted: Vec<HoistedHeader>,
    /// The record covering each block's accesses to each collection.
    covering: HashMap<(usize, Local), usize>,
    current_block: Cell<Option<usize>>,
}

impl StableHeaders {
    /// No header read once: what code with no body of its own — a thunk, a
    /// destructor — translates against.
    pub fn none() -> Self {
        Self::default()
    }

    /// The headers `body` reads once, each given its two variables. MIR
    /// guarantees one read per block and collection, and that its
    /// preheader dominates its blocks; the verifier checks both.
    pub(crate) fn declare(
        builder: &mut FunctionBuilder,
        body: &Body,
        ptr_type: cl_types::Type,
    ) -> Self {
        let mut headers = Self::default();
        for (index, record) in stable_collections(body).iter().enumerate() {
            headers.hoisted.push(HoistedHeader {
                local: record.local,
                preheader: record.preheader.0,
                data: builder.declare_var(ptr_type),
                len: builder.declare_var(ptr_type),
            });
            for block in &record.blocks {
                headers
                    .covering
                    .entry((block.0, record.local))
                    .or_insert(index);
            }
        }
        headers
    }

    /// The read of `local`'s header made in the block being translated.
    fn read_here(&self, local: Local) -> Option<HoistedHeader> {
        let block = self.current_block.get()?;
        (self.hoisted.iter())
            .find(|hoisted| hoisted.preheader == block && hoisted.local == local)
            .copied()
    }

    /// Note that the MIR block `block` is being translated.
    pub fn enter_block(&self, block: usize) {
        self.current_block.set(Some(block));
    }

    /// The header of the collection `local` holds, read before the loop the
    /// block being translated belongs to, if one was.
    pub(crate) fn hoisted(
        &self,
        builder: &mut FunctionBuilder,
        local: Local,
    ) -> Option<ElementHeader> {
        let block = self.current_block.get()?;
        let index = *self.covering.get(&(block, local))?;
        let hoisted = self.hoisted.get(index)?;
        Some(ElementHeader {
            data: builder.use_var(hoisted.data),
            len: builder.use_var(hoisted.len),
        })
    }
}

impl FunctionTranslator<'_> {
    /// Read the header of `collection`, where a `ReadHeader` statement of
    /// the MIR block being translated reads it.
    pub(crate) fn read_stable_headers(
        builder: &mut FunctionBuilder,
        collection: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let Some(hoisted) = type_ctx.stable_headers.read_here(collection.local) else {
            return Err(CodegenError::Internal(format!(
                "`{collection}` is read once in a block no read of it names"
            )));
        };
        let var = locals
            .get(&hoisted.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", hoisted.local)))?;
        let base = builder.use_var(*var);
        Self::read_stable_header(builder, base, hoisted, type_ctx)
    }

    /// Define `hoisted`'s variables from the header at `base`: zero for a
    /// null collection, its data pointer and length otherwise, checking its
    /// slot size first when its elements are held at a stride of their own
    /// and it has any.
    fn read_stable_header(
        builder: &mut FunctionBuilder,
        base: Value,
        hoisted: HoistedHeader,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let base_type = type_ctx
            .local_types
            .get(hoisted.local.0)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", hoisted.local)))?;
        let stride = Self::resolve_collection_elem_type(base_type)
            .and_then(|kind| inline_stride(kind, type_ctx));
        let load_block = builder.create_block();
        let null_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(base, load_block, &[], null_block, &[]);

        builder.switch_to_block(null_block);
        builder.seal_block(null_block);
        let zero = builder.ins().iconst(ptr_type, 0);
        builder.def_var(hoisted.data, zero);
        builder.def_var(hoisted.len, zero);
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(load_block);
        builder.seal_block(load_block);
        let header = Self::load_element_header(builder, base, base_type, None, ptr_type);
        builder.def_var(hoisted.data, header.data);
        builder.def_var(hoisted.len, header.len);
        if let Some(stride) = stride {
            let check_block = builder.create_block();
            builder
                .ins()
                .brif(header.len, check_block, &[], merge_block, &[]);
            builder.switch_to_block(check_block);
            builder.seal_block(check_block);
            Self::trap_unless_slots_match_stride(builder, base, base_type, stride, ptr_type);
        }
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(())
    }
}
