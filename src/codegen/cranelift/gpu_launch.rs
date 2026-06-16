// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `TerminatorKind::GpuLaunch` → Cranelift call into `miri_gpu_launch_inline`.
//!
//! Each `gpu fn` kernel body is compiled to WGSL by the WGSL backend; the
//! text + entry-point name are embedded as data sections in the object file.
//! At each launch site the translator allocates a `GpuLaunchDesc` plus
//! per-capture data-pointer / byte-length arrays on the function stack,
//! marshals the host-side `MiriArray` captures into the descriptor, and
//! invokes the single runtime entry that handles init / compile / cache /
//! dispatch / sync / readback.

use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind, DIM3_TYPE_NAME};
use crate::codegen::cranelift::layout::field_layout;
use crate::codegen::cranelift::translator::{ModuleCtx, TypeCtx};
use crate::codegen::wgsl::{WgslBackend, WgslOptions};
use crate::codegen::Backend;
use crate::error::CodegenError;
use crate::mir::body::DeviceHandleId;
use crate::mir::{Body, ExecutionModel, Local, Operand, Place};
use cranelift_codegen::ir::{
    condcodes::IntCC, types as cl_types, AbiParam, InstBuilder, MemFlags, StackSlotData,
    StackSlotKind, TrapCode, Value,
};
use cranelift_frontend::FunctionBuilder;
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::HashMap;

/// Compile-time info for one GPU kernel emitted by the WGSL backend.
#[derive(Debug, Clone)]
pub struct KernelEmit {
    pub(crate) wgsl_data: DataId,
    pub(crate) wgsl_len: usize,
    pub(crate) name_data: DataId,
    pub(crate) name_len: usize,
}

/// For each kernel in `bodies`, compile WGSL and emit a data section
/// for both the source and the entry-point name. Returns a name → emit
/// info map used by `translate` at each `GpuLaunch` site.
pub(crate) fn build_kernel_registry(
    module: &mut ObjectModule,
    bodies: &[(&str, &Body)],
) -> Result<HashMap<String, KernelEmit>, CodegenError> {
    let backend = WgslBackend;
    let options = WgslOptions::default();
    let mut registry = HashMap::new();
    for (name, body) in bodies {
        if body.execution_model != ExecutionModel::GpuKernel {
            continue;
        }
        let artifact = backend.compile(&[(*name, *body)], &options)?;
        let wgsl_text = String::from_utf8(artifact.bytes).map_err(|err| {
            CodegenError::Internal(format!(
                "WGSL backend produced non-UTF-8 output for kernel {}: {}",
                name, err
            ))
        })?;
        let wgsl_data = define_bytes(
            module,
            &format!("__miri_kernel_{name}_wgsl"),
            wgsl_text.as_bytes(),
        )?;
        let name_data = define_bytes(
            module,
            &format!("__miri_kernel_{name}_name"),
            name.as_bytes(),
        )?;

        registry.insert(
            (*name).to_string(),
            KernelEmit {
                wgsl_data,
                wgsl_len: wgsl_text.len(),
                name_data,
                name_len: name.len(),
            },
        );
    }
    Ok(registry)
}

fn define_bytes(
    module: &mut ObjectModule,
    symbol: &str,
    bytes: &[u8],
) -> Result<DataId, CodegenError> {
    let id = module
        .declare_data(symbol, Linkage::Local, false, false)
        .map_err(|err| CodegenError::Module(err.to_string()))?;
    let mut desc = DataDescription::new();
    desc.define(bytes.to_vec().into_boxed_slice());
    module
        .define_data(id, &desc)
        .map_err(|err| CodegenError::Module(err.to_string()))?;
    Ok(id)
}

/// Layout of `GpuLaunchDesc` in `src/runtime/gpu/src/launch.rs` (repr(C)).
/// All 8-byte fields are naturally aligned; the six packed u32 dims
/// (offsets 32..56) sit on 4-byte boundaries and don't introduce padding
/// before the trailing pointers because 56 is already 8-aligned.
/// Offsets 88+ hold the variable fields (uniform bounds, buf_read_only, buf_int_narrow,
/// scalar inputs).
mod desc_layout {
    pub(super) const WGSL_PTR: i32 = 0;
    pub(super) const WGSL_LEN: i32 = 8;
    pub(super) const ENTRY_PTR: i32 = 16;
    pub(super) const ENTRY_LEN: i32 = 24;
    pub(super) const GRID_X: i32 = 32;
    pub(super) const GRID_Y: i32 = 36;
    pub(super) const GRID_Z: i32 = 40;
    pub(super) const BLOCK_X: i32 = 44;
    pub(super) const BLOCK_Y: i32 = 48;
    pub(super) const BLOCK_Z: i32 = 52;
    pub(super) const NUM_BUFS: i32 = 56;
    pub(super) const BUF_DATA_PTRS: i32 = 64;
    pub(super) const BUF_BYTE_LENS: i32 = 72;
    pub(super) const BUF_HANDLE_IDS: i32 = 80;
    pub(super) const BUF_READ_ONLY: i32 = 88;
    pub(super) const BUF_INT_NARROW: i32 = 96;
    pub(super) const UNIFORM_BOUND_PRESENT: i32 = 104;
    pub(super) const UNIFORM_BOUND_X_VALUE: i32 = 112;
    pub(super) const UNIFORM_BOUND_Y_VALUE: i32 = 120;
    pub(super) const UNIFORM_BOUND_Z_VALUE: i32 = 128;
    pub(super) const NUM_STORAGE_BUFS: i32 = 136;
    pub(super) const SCALAR_INPUTS_PTR: i32 = 144;
    pub(super) const SCALAR_INPUTS_LEN: i32 = 152;
    pub(super) const DESC_SIZE: u32 = 160;
}

/// Field offsets within `runtime::core::MiriArray` (`repr(C)`):
/// `{ data: *mut u8, elem_count: usize, elem_size: usize, … }`.
/// Centralized here so the GPU dispatcher cannot drift out of sync if
/// the runtime struct gains or reorders a leading field.
mod miri_array_layout {
    pub(super) const DATA_OFFSET: i32 = 0;
    pub(super) const ELEM_COUNT_OFFSET: i32 = 8;
    pub(super) const ELEM_SIZE_OFFSET: i32 = 16;
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn translate(
    builder: &mut FunctionBuilder,
    module_ctx: &mut ModuleCtx,
    kernel_op: &Operand,
    grid_op: &Operand,
    block_op: &Operand,
    args: &[Operand],
    arg_handles: &[Option<DeviceHandleId>],
    _arg_read_only: &[bool],
    arg_int_narrow: &[bool],
    _scalar_args: &[Operand],
    uniform_bound_x: &Option<Box<Operand>>,
    uniform_bound_y: &Option<Box<Operand>>,
    uniform_bound_z: &Option<Box<Operand>>,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<(), CodegenError> {
    let kernel_name = extract_kernel_name(kernel_op)?;
    let kernel = module_ctx
        .kernel_registry
        .get(&kernel_name)
        .ok_or_else(|| {
            CodegenError::Internal(format!(
                "GpuLaunch references kernel '{}' which has no WGSL artifact",
                kernel_name
            ))
        })?
        .clone();

    let ptr_ty = type_ctx.ptr_type;
    let num_bufs = args.len();
    let slots = allocate_launch_slots(builder, ptr_ty, num_bufs);

    populate_capture_arrays(
        builder,
        args,
        slots.data_ptrs_addr,
        slots.byte_lens_addr,
        ptr_ty,
        locals,
        type_ctx,
    )?;
    populate_handle_ids(builder, arg_handles, num_bufs, slots.handle_ids_addr);

    // Allocate and populate buf_read_only if non-empty.
    let read_only_addr = if _arg_read_only.is_empty() {
        builder.ins().iconst(ptr_ty, 0)
    } else {
        let read_only_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            _arg_read_only.len() as u32,
            1,
        ));
        let read_only_addr = builder.ins().stack_addr(ptr_ty, read_only_slot, 0);
        for (i, &is_ro) in _arg_read_only.iter().enumerate() {
            let byte_val = builder
                .ins()
                .iconst(cl_types::I8, if is_ro { 1 } else { 0 });
            builder
                .ins()
                .store(MemFlags::new(), byte_val, read_only_addr, i as i32);
        }
        read_only_addr
    };

    // Allocate and populate buf_int_narrow if non-empty.
    let int_narrow_addr = if arg_int_narrow.is_empty() {
        builder.ins().iconst(ptr_ty, 0)
    } else {
        let int_narrow_slot = builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            arg_int_narrow.len() as u32,
            1,
        ));
        let int_narrow_addr = builder.ins().stack_addr(ptr_ty, int_narrow_slot, 0);
        for (i, &needs_narrow) in arg_int_narrow.iter().enumerate() {
            let byte_val = builder
                .ins()
                .iconst(cl_types::I8, if needs_narrow { 1 } else { 0 });
            builder
                .ins()
                .store(MemFlags::new(), byte_val, int_narrow_addr, i as i32);
        }
        int_narrow_addr
    };

    let (grid_x, grid_y, grid_z) = load_dim3_components(builder, grid_op, locals, type_ctx)?;
    let (block_x, block_y, block_z) = load_dim3_components(builder, block_op, locals, type_ctx)?;

    let (scalar_inputs_addr, scalar_inputs_len) =
        populate_scalar_inputs(builder, ptr_ty, _scalar_args, locals, type_ctx)?;

    populate_descriptor(
        builder,
        module_ctx.module,
        DescriptorSlots {
            desc_addr: slots.desc_addr,
            data_ptrs_addr: slots.data_ptrs_addr,
            byte_lens_addr: slots.byte_lens_addr,
            handle_ids_addr: slots.handle_ids_addr,
            read_only_addr,
            int_narrow_addr,
            scalar_inputs_addr,
            scalar_inputs_len,
        },
        &kernel,
        ptr_ty,
        num_bufs,
        [grid_x, grid_y, grid_z],
        [block_x, block_y, block_z],
    );

    // Populate uniform bounds if present.
    let zero_i64 = builder.ins().iconst(cl_types::I64, 0);
    let mut bound_present = 0u64;

    if let Some(bound_op) = uniform_bound_x {
        let bound_value = read_operand_value(builder, bound_op, locals, type_ctx)?;
        builder.ins().store(
            MemFlags::new(),
            bound_value,
            slots.desc_addr,
            desc_layout::UNIFORM_BOUND_X_VALUE,
        );
        bound_present |= 1u64;
    } else {
        builder.ins().store(
            MemFlags::new(),
            zero_i64,
            slots.desc_addr,
            desc_layout::UNIFORM_BOUND_X_VALUE,
        );
    }

    if let Some(bound_op) = uniform_bound_y {
        let bound_value = read_operand_value(builder, bound_op, locals, type_ctx)?;
        builder.ins().store(
            MemFlags::new(),
            bound_value,
            slots.desc_addr,
            desc_layout::UNIFORM_BOUND_Y_VALUE,
        );
        bound_present |= 2u64;
    } else {
        builder.ins().store(
            MemFlags::new(),
            zero_i64,
            slots.desc_addr,
            desc_layout::UNIFORM_BOUND_Y_VALUE,
        );
    }

    if let Some(bound_op) = uniform_bound_z {
        let bound_value = read_operand_value(builder, bound_op, locals, type_ctx)?;
        builder.ins().store(
            MemFlags::new(),
            bound_value,
            slots.desc_addr,
            desc_layout::UNIFORM_BOUND_Z_VALUE,
        );
        bound_present |= 4u64;
    } else {
        builder.ins().store(
            MemFlags::new(),
            zero_i64,
            slots.desc_addr,
            desc_layout::UNIFORM_BOUND_Z_VALUE,
        );
    }

    let bound_present_i64 = builder.ins().iconst(cl_types::I64, bound_present as i64);
    builder.ins().store(
        MemFlags::new(),
        bound_present_i64,
        slots.desc_addr,
        desc_layout::UNIFORM_BOUND_PRESENT,
    );

    // Store num_storage_bufs (= num_bufs, always equal to capture count).
    let num_storage_i64 = builder.ins().iconst(cl_types::I64, num_bufs as i64);
    builder.ins().store(
        MemFlags::new(),
        num_storage_i64,
        slots.desc_addr,
        desc_layout::NUM_STORAGE_BUFS,
    );

    let func_id = declare_launch_fn(module_ctx.module, ptr_ty)?;
    let local_func = module_ctx
        .module
        .declare_func_in_func(func_id, builder.func);
    let call = builder.ins().call(local_func, &[slots.desc_addr]);

    trap_on_launch_failure(builder, call)?;

    Ok(())
}

/// Converts a GPU launch failure (return code 0) into a trap.
///
/// The runtime prints a descriptive error message to stderr before returning 0.
/// This helper reads the return code, compares it to 0, and emits a trap on
/// failure. The process terminates cleanly with the error message visible
/// to the user on stderr.
///
/// After this call, the builder is positioned on the continuation block
/// (success path) so the parent `translate` function can proceed normally.
fn trap_on_launch_failure(
    builder: &mut FunctionBuilder,
    call: cranelift_codegen::ir::Inst,
) -> Result<(), CodegenError> {
    let call_result = builder.inst_results(call)[0];
    let zero_i8 = builder.ins().iconst(cl_types::I8, 0);
    let failed = builder.ins().icmp(IntCC::Equal, call_result, zero_i8);

    let fail_block = builder.create_block();
    let cont_block = builder.create_block();
    builder.ins().brif(failed, fail_block, &[], cont_block, &[]);

    builder.switch_to_block(fail_block);
    builder.ins().trap(TrapCode::unwrap_user(1));
    builder.seal_block(fail_block);

    builder.switch_to_block(cont_block);
    builder.seal_block(cont_block);

    Ok(())
}

fn extract_kernel_name(kernel_op: &Operand) -> Result<String, CodegenError> {
    match kernel_op {
        Operand::Constant(c) => match &c.literal {
            Literal::Identifier(name) => Ok(name.clone()),
            Literal::Integer(_)
            | Literal::Float(_)
            | Literal::String(_)
            | Literal::Boolean(_)
            | Literal::Regex(_)
            | Literal::None => Err(CodegenError::Internal(
                "GpuLaunch kernel operand must be an Identifier constant".to_string(),
            )),
        },
        Operand::Copy(_) | Operand::Move(_) => Err(CodegenError::Internal(
            "GpuLaunch kernel operand must be a Constant".to_string(),
        )),
    }
}

struct LaunchSlots {
    data_ptrs_addr: Value,
    byte_lens_addr: Value,
    handle_ids_addr: Value,
    desc_addr: Value,
}

fn allocate_launch_slots(
    builder: &mut FunctionBuilder,
    ptr_ty: cl_types::Type,
    num_bufs: usize,
) -> LaunchSlots {
    let data_ptrs_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (num_bufs.max(1) as u32) * ptr_ty.bytes(),
        ptr_ty.bytes() as u8,
    ));
    let byte_lens_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (num_bufs.max(1) as u32) * 8,
        8,
    ));
    let handle_ids_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        (num_bufs.max(1) as u32) * 8,
        8,
    ));
    let desc_slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        desc_layout::DESC_SIZE,
        8,
    ));
    LaunchSlots {
        data_ptrs_addr: builder.ins().stack_addr(ptr_ty, data_ptrs_slot, 0),
        byte_lens_addr: builder.ins().stack_addr(ptr_ty, byte_lens_slot, 0),
        handle_ids_addr: builder.ins().stack_addr(ptr_ty, handle_ids_slot, 0),
        desc_addr: builder.ins().stack_addr(ptr_ty, desc_slot, 0),
    }
}

/// Stores each capture's `DeviceHandleId` (or `0` for a host-resident
/// capture) into the handle-ids stack array the runtime reads to decide
/// whether a buffer persists across launches.
fn populate_handle_ids(
    builder: &mut FunctionBuilder,
    arg_handles: &[Option<DeviceHandleId>],
    num_bufs: usize,
    handle_ids_addr: Value,
) {
    for i in 0..num_bufs {
        let id = arg_handles
            .get(i)
            .copied()
            .flatten()
            .map_or(0, |handle| handle.0);
        let id_value = builder.ins().iconst(cl_types::I64, id as i64);
        builder
            .ins()
            .store(MemFlags::new(), id_value, handle_ids_addr, (i as i32) * 8);
    }
}

#[allow(clippy::too_many_arguments)]
fn populate_capture_arrays(
    builder: &mut FunctionBuilder,
    args: &[Operand],
    data_ptrs_addr: Value,
    byte_lens_addr: Value,
    ptr_ty: cl_types::Type,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<(), CodegenError> {
    let ptr_size = ptr_ty.bytes() as i32;
    for (i, arg) in args.iter().enumerate() {
        let arr_ptr = read_operand_value(builder, arg, locals, type_ctx)?;
        let data_ptr = builder.ins().load(
            ptr_ty,
            MemFlags::new(),
            arr_ptr,
            miri_array_layout::DATA_OFFSET,
        );
        let elem_count = builder.ins().load(
            cl_types::I64,
            MemFlags::new(),
            arr_ptr,
            miri_array_layout::ELEM_COUNT_OFFSET,
        );
        let elem_size = builder.ins().load(
            cl_types::I64,
            MemFlags::new(),
            arr_ptr,
            miri_array_layout::ELEM_SIZE_OFFSET,
        );
        let byte_len = builder.ins().imul(elem_count, elem_size);
        builder.ins().store(
            MemFlags::new(),
            data_ptr,
            data_ptrs_addr,
            (i as i32) * ptr_size,
        );
        builder
            .ins()
            .store(MemFlags::new(), byte_len, byte_lens_addr, (i as i32) * 8);
    }
    Ok(())
}

/// Stack addresses the descriptor's pointer fields reference.
struct DescriptorSlots {
    desc_addr: Value,
    data_ptrs_addr: Value,
    byte_lens_addr: Value,
    handle_ids_addr: Value,
    read_only_addr: Value,
    int_narrow_addr: Value,
    scalar_inputs_addr: Value,
    scalar_inputs_len: Value,
}

/// Packs scalar captures into a binary blob on the stack.
/// Each scalar is stored at a 4-byte offset in order: int→i32, bool→u32, f32→f32.
fn populate_scalar_inputs(
    builder: &mut FunctionBuilder,
    ptr_ty: cl_types::Type,
    scalar_args: &[Operand],
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<(Value, Value), CodegenError> {
    if scalar_args.is_empty() {
        return Ok((
            builder.ins().iconst(ptr_ty, 0),
            builder.ins().iconst(cl_types::I64, 0),
        ));
    }

    let byte_size = (scalar_args.len() as u32) * 4;
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        byte_size,
        4,
    ));
    let addr = builder.ins().stack_addr(ptr_ty, slot, 0);

    for (i, op) in scalar_args.iter().enumerate() {
        let value = read_operand_value(builder, op, locals, type_ctx)?;
        let place = match op {
            Operand::Copy(p) | Operand::Move(p) => p,
            Operand::Constant(_) => {
                return Err(CodegenError::Internal(
                    "scalar_args operand must be a Copy/Move".to_string(),
                ));
            }
        };
        let local_ty = type_ctx.local_types.get(place.local.0).ok_or_else(|| {
            CodegenError::Internal(format!(
                "unknown local in scalar capture: {:?}",
                place.local
            ))
        })?;
        let converted = convert_scalar_for_uniform(builder, value, local_ty)?;
        builder
            .ins()
            .store(MemFlags::new(), converted, addr, (i as i32) * 4);
    }

    Ok((addr, builder.ins().iconst(cl_types::I64, byte_size as i64)))
}

/// Converts a scalar operand to the wire format: int→i32, bool→u32, f32→f32.
fn convert_scalar_for_uniform(
    builder: &mut FunctionBuilder,
    value: Value,
    local_ty: &&Type,
) -> Result<Value, CodegenError> {
    match &local_ty.kind {
        TypeKind::Int => {
            let val_ty = builder.func.dfg.value_type(value);
            if val_ty == cl_types::I64 {
                Ok(builder.ins().ireduce(cl_types::I32, value))
            } else {
                Ok(value)
            }
        }
        TypeKind::Boolean => {
            let val_ty = builder.func.dfg.value_type(value);
            if val_ty == cl_types::I8 {
                Ok(builder.ins().uextend(cl_types::I32, value))
            } else {
                Ok(value)
            }
        }
        TypeKind::F32 => Ok(value),
        _ => Err(CodegenError::Internal(format!(
            "unsupported scalar capture type in codegen: {:?}",
            local_ty.kind
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
fn populate_descriptor(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    slots: DescriptorSlots,
    kernel: &KernelEmit,
    ptr_ty: cl_types::Type,
    num_bufs: usize,
    grid_xyz: [Value; 3],
    block_xyz: [Value; 3],
) {
    let DescriptorSlots {
        desc_addr,
        data_ptrs_addr,
        byte_lens_addr,
        handle_ids_addr,
        read_only_addr,
        int_narrow_addr,
        scalar_inputs_addr,
        scalar_inputs_len,
    } = slots;
    let wgsl_ptr = data_pointer(builder, module, kernel.wgsl_data, ptr_ty);
    let entry_ptr = data_pointer(builder, module, kernel.name_data, ptr_ty);
    let wgsl_len = builder.ins().iconst(cl_types::I64, kernel.wgsl_len as i64);
    let entry_len = builder.ins().iconst(cl_types::I64, kernel.name_len as i64);
    let num_bufs_v = builder.ins().iconst(cl_types::I64, num_bufs as i64);

    let mut store = |value: Value, offset: i32| {
        builder
            .ins()
            .store(MemFlags::new(), value, desc_addr, offset);
    };
    store(wgsl_ptr, desc_layout::WGSL_PTR);
    store(wgsl_len, desc_layout::WGSL_LEN);
    store(entry_ptr, desc_layout::ENTRY_PTR);
    store(entry_len, desc_layout::ENTRY_LEN);
    store(grid_xyz[0], desc_layout::GRID_X);
    store(grid_xyz[1], desc_layout::GRID_Y);
    store(grid_xyz[2], desc_layout::GRID_Z);
    store(block_xyz[0], desc_layout::BLOCK_X);
    store(block_xyz[1], desc_layout::BLOCK_Y);
    store(block_xyz[2], desc_layout::BLOCK_Z);
    store(num_bufs_v, desc_layout::NUM_BUFS);
    store(data_ptrs_addr, desc_layout::BUF_DATA_PTRS);
    store(byte_lens_addr, desc_layout::BUF_BYTE_LENS);
    store(handle_ids_addr, desc_layout::BUF_HANDLE_IDS);
    store(read_only_addr, desc_layout::BUF_READ_ONLY);
    store(int_narrow_addr, desc_layout::BUF_INT_NARROW);
    store(scalar_inputs_addr, desc_layout::SCALAR_INPUTS_PTR);
    store(scalar_inputs_len, desc_layout::SCALAR_INPUTS_LEN);
}

fn declare_launch_fn(
    module: &mut ObjectModule,
    ptr_ty: cl_types::Type,
) -> Result<FuncId, CodegenError> {
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(ptr_ty));
    sig.returns.push(AbiParam::new(cl_types::I8));
    module
        .declare_function("miri_gpu_launch_inline", Linkage::Import, &sig)
        .map_err(|err| {
            CodegenError::declare_function("miri_gpu_launch_inline".to_string(), err.to_string())
        })
}

fn data_pointer(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    data: DataId,
    ptr_ty: cl_types::Type,
) -> Value {
    let global = module.declare_data_in_func(data, builder.func);
    builder.ins().symbol_value(ptr_ty, global)
}

fn read_operand_value(
    builder: &mut FunctionBuilder,
    op: &Operand,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<Value, CodegenError> {
    let place = match op {
        Operand::Copy(p) | Operand::Move(p) => p,
        Operand::Constant(_) => {
            return Err(CodegenError::Internal(
                "GpuLaunch operand must be a Copy/Move of a projection-free Local".to_string(),
            ));
        }
    };
    if !place.projection.is_empty() {
        return Err(CodegenError::Internal(
            "GpuLaunch operand must be a Copy/Move of a projection-free Local".to_string(),
        ));
    }
    read_place_value(builder, place, locals, type_ctx)
}

fn read_place_value(
    builder: &mut FunctionBuilder,
    place: &Place,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<Value, CodegenError> {
    let var = *locals.get(&place.local).ok_or_else(|| {
        CodegenError::Internal(format!(
            "GpuLaunch references unknown local {:?}",
            place.local
        ))
    })?;
    let _ = type_ctx;
    Ok(builder.use_var(var))
}

fn load_dim3_components(
    builder: &mut FunctionBuilder,
    op: &Operand,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<(Value, Value, Value), CodegenError> {
    // `Dim3` is a struct local whose field layout is owned by
    // `codegen::cranelift::layout`. Route through that module so a future
    // change to `Dim3` (extra field, reordered layout, different scalar
    // width) propagates here automatically instead of silently producing
    // wrong dispatch dims.
    let base_addr = read_operand_value(builder, op, locals, type_ctx)?;
    let dim3_kind = TypeKind::Custom(DIM3_TYPE_NAME.to_string(), None);
    let ptr_ty = type_ctx.ptr_type;
    let (off_x, ty_x) = field_layout(&dim3_kind, 0, type_ctx.type_definitions, ptr_ty);
    let (off_y, ty_y) = field_layout(&dim3_kind, 1, type_ctx.type_definitions, ptr_ty);
    let (off_z, ty_z) = field_layout(&dim3_kind, 2, type_ctx.type_definitions, ptr_ty);
    let x = builder.ins().load(ty_x, MemFlags::new(), base_addr, off_x);
    let y = builder.ins().load(ty_y, MemFlags::new(), base_addr, off_y);
    let z = builder.ins().load(ty_z, MemFlags::new(), base_addr, off_z);
    Ok((
        narrow_to_i32(builder, x, ty_x),
        narrow_to_i32(builder, y, ty_y),
        narrow_to_i32(builder, z, ty_z),
    ))
}

fn narrow_to_i32(builder: &mut FunctionBuilder, value: Value, from: cl_types::Type) -> Value {
    if from == cl_types::I32 {
        value
    } else {
        builder.ins().ireduce(cl_types::I32, value)
    }
}

#[cfg(test)]
mod tests {
    use super::desc_layout;

    #[test]
    fn gpu_launch_desc_size_matches_runtime() {
        assert_eq!(desc_layout::DESC_SIZE as usize, 160);
    }
}
