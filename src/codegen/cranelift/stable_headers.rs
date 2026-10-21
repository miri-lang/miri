// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The collection headers a function reads once per loop.
//!
//! MIR records, on `Body::stable_collections`, each array or list a loop
//! leaves the header of unchanged, with a preheader every entry into the loop
//! passes through. At the end of that preheader the header is read once:
//! null-guarded, so a collection never allocated reads as no data and no
//! elements, as `Len` reads it; and, for elements held at a stride of their
//! own, with the slot-size check run only when there is an element to check.
//! An element access or a length inside the loop rooted at the collection
//! then reads the two words from the variables those loads defined.
//!
//! Where a collection is recorded over nested loops, the outermost record
//! covers the blocks they share, so the header is read once for the whole
//! nest. Each record's preheader must dominate its blocks; a record that does
//! not would read a variable some path never defined, and is reported rather
//! than compiled.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use crate::codegen::cranelift::collection_header::ElementHeader;
use crate::codegen::cranelift::translator::{inline_stride, FunctionTranslator, TypeCtx};
use crate::error::CodegenError;
use crate::mir::{Body, Local, StableCollection};

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

    /// The headers `body` records, each given its two variables, after
    /// checking that every record's preheader dominates its blocks.
    pub(crate) fn declare(
        builder: &mut FunctionBuilder,
        body: &Body,
        ptr_type: cl_types::Type,
    ) -> Result<Self, CodegenError> {
        let mut headers = Self::default();
        let mut widest: HashMap<(usize, Local), usize> = HashMap::new();
        for (index, record) in body.stable_collections.iter().enumerate() {
            check_preheader_dominates(body, record)?;
            headers.hoisted.push(HoistedHeader {
                local: record.local,
                preheader: record.preheader.0,
                data: builder.declare_var(ptr_type),
                len: builder.declare_var(ptr_type),
            });
            for block in &record.blocks {
                let key = (block.0, record.local);
                let is_wider = widest
                    .get(&key)
                    .is_none_or(|covered| record.blocks.len() > *covered);
                if is_wider {
                    widest.insert(key, record.blocks.len());
                    headers.covering.insert(key, index);
                }
            }
        }
        Ok(headers)
    }

    /// Note that the MIR block `block` is being translated.
    pub(crate) fn enter_block(&self, block: usize) {
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

    fn read_in(&self, block: usize) -> impl Iterator<Item = &HoistedHeader> {
        self.hoisted
            .iter()
            .filter(move |hoisted| hoisted.preheader == block)
    }
}

/// Fails unless every path from the entry into a block of `record` passes
/// through its preheader.
fn check_preheader_dominates(body: &Body, record: &StableCollection) -> Result<(), CodegenError> {
    let preheader = record.preheader.0;
    let mut reached: HashSet<usize> = HashSet::from([0]);
    let mut pending = vec![0];
    while let Some(block) = pending.pop() {
        if block == preheader {
            continue;
        }
        let successors = (body.basic_blocks.get(block))
            .and_then(|data| data.terminator.as_ref())
            .map(|terminator| terminator.successors())
            .unwrap_or_default();
        pending.extend(
            successors
                .into_iter()
                .map(|successor| successor.0)
                .filter(|successor| reached.insert(*successor)),
        );
    }
    match record
        .blocks
        .iter()
        .find(|block| reached.contains(&block.0))
    {
        Some(block) => Err(CodegenError::Internal(format!(
            "`{}` is read once in {} for the loop at {}, which does not dominate {block}",
            record.local, record.preheader, record.header
        ))),
        None => Ok(()),
    }
}

impl FunctionTranslator<'_> {
    /// Read, at the end of the MIR block `block`, the header of every
    /// collection recorded with it as preheader.
    pub(crate) fn read_stable_headers(
        builder: &mut FunctionBuilder,
        block: usize,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let headers: Vec<HoistedHeader> = type_ctx.stable_headers.read_in(block).copied().collect();
        for hoisted in headers {
            let var = locals.get(&hoisted.local).ok_or_else(|| {
                CodegenError::Internal(format!("Unknown local: {:?}", hoisted.local))
            })?;
            let base = builder.use_var(*var);
            Self::read_stable_header(builder, base, hoisted, type_ctx)?;
        }
        Ok(())
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
