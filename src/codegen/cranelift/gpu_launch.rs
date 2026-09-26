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

use crate::ast::gpu_wire::{buffer_conversion, scalar_capture_wire, WireConversion};
use crate::ast::literal::Literal;
use crate::ast::types::{Type, TypeKind, DIM3_TYPE_NAME};
use crate::codegen::cranelift::layout::field_layout;
use crate::codegen::cranelift::translator::{ModuleCtx, TypeCtx};
use crate::codegen::wgsl::{WgslBackend, WgslOptions};
use crate::codegen::Backend;
use crate::error::CodegenError;
use crate::mir::body::DeviceHandleId;
use crate::mir::symbol::{KernelDatum, Symbol};
use crate::mir::{Body, ExecutionModel, GpuLaunchArgs, Local, Operand, Place};
use crate::runtime_fns::rt;
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

    // GpuDevice helper bodies (user functions called from kernels) are emitted
    // into every kernel module so the kernel's calls resolve. Unused helpers are
    // harmless dead functions in WGSL.
    let helpers: Vec<(&str, &Body)> = bodies
        .iter()
        .filter(|(_, b)| b.execution_model == ExecutionModel::GpuDevice)
        .map(|(n, b)| (*n, *b))
        .collect();

    for (name, body) in bodies {
        if body.execution_model != ExecutionModel::GpuKernel {
            continue;
        }
        let mut module_bodies: Vec<(&str, &Body)> = Vec::with_capacity(1 + helpers.len());
        module_bodies.extend_from_slice(&helpers);
        module_bodies.push((*name, *body));
        let artifact = backend.compile(&module_bodies, &options)?;
        let wgsl_text = String::from_utf8(artifact.bytes).map_err(|err| {
            CodegenError::Internal(format!(
                "WGSL backend produced non-UTF-8 output for kernel {}: {}",
                name, err
            ))
        })?;
        let wgsl_data = define_bytes(
            module,
            &Symbol::kernel_datum(name, KernelDatum::Wgsl).link_name(),
            wgsl_text.as_bytes(),
        )?;
        let name_data = define_bytes(
            module,
            &Symbol::kernel_datum(name, KernelDatum::Name).link_name(),
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
/// Offsets 88+ hold the variable fields (uniform bounds, buf_read_only, buf_wire_conversion,
/// scalar inputs, and the runtime range-start uniforms).
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
    pub(super) const BUF_WIRE_CONVERSION: i32 = 96;
    pub(super) const UNIFORM_BOUND_PRESENT: i32 = 104;
    pub(super) const UNIFORM_BOUND_X_VALUE: i32 = 112;
    pub(super) const UNIFORM_BOUND_Y_VALUE: i32 = 120;
    pub(super) const UNIFORM_BOUND_Z_VALUE: i32 = 128;
    pub(super) const SCALAR_INPUTS_PTR: i32 = 136;
    pub(super) const SCALAR_INPUTS_LEN: i32 = 144;
    pub(super) const UNIFORM_START_X_VALUE: i32 = 152;
    pub(super) const UNIFORM_START_Y_VALUE: i32 = 160;
    pub(super) const UNIFORM_START_Z_VALUE: i32 = 168;
    pub(super) const DESC_SIZE: u32 = 176;
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
    launch_args: &GpuLaunchArgs,
    _scalar_args: &[Operand],
    uniform_bound_x: &Option<Box<Operand>>,
    uniform_bound_y: &Option<Box<Operand>>,
    uniform_bound_z: &Option<Box<Operand>>,
    uniform_start_x: &Option<Box<Operand>>,
    uniform_start_y: &Option<Box<Operand>>,
    uniform_start_z: &Option<Box<Operand>>,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<(), CodegenError> {
    let args = launch_args.args();
    let arg_handles = launch_args.arg_handles();
    let _arg_read_only = launch_args.arg_read_only();
    let arg_needs_conversion = launch_args.arg_int_narrow();
    // The parallel per-capture vectors are written to the `#[repr(C)]`
    // `GpuLaunchDesc` by index below; `GpuLaunchArgs` guarantees they are
    // equal-length at construction, but assert the contract here so a future
    // change that bypasses the builder fails at codegen, not at the GPU driver.
    debug_assert_eq!(args.len(), arg_handles.len());
    debug_assert_eq!(args.len(), _arg_read_only.len());
    debug_assert_eq!(args.len(), arg_needs_conversion.len());

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

    let read_only_bytes: Vec<u8> = _arg_read_only
        .iter()
        .map(|&is_ro| u8::from(is_ro))
        .collect();
    let read_only_addr = store_byte_array(builder, ptr_ty, &read_only_bytes);
    let conversion_codes = buffer_conversion_codes(args, arg_needs_conversion, type_ctx)?;
    let wire_conversion_addr = store_byte_array(builder, ptr_ty, &conversion_codes);

    let (grid_x, grid_y, grid_z) = load_dim3_components(builder, grid_op, locals, type_ctx)?;
    let (block_x, block_y, block_z) = load_dim3_components(builder, block_op, locals, type_ctx)?;

    let (scalar_inputs_addr, scalar_inputs_len) =
        populate_scalar_inputs(builder, module_ctx.module, _scalar_args, locals, type_ctx)?;

    populate_descriptor(
        builder,
        module_ctx.module,
        DescriptorSlots {
            desc_addr: slots.desc_addr,
            data_ptrs_addr: slots.data_ptrs_addr,
            byte_lens_addr: slots.byte_lens_addr,
            handle_ids_addr: slots.handle_ids_addr,
            read_only_addr,
            wire_conversion_addr,
            scalar_inputs_addr,
            scalar_inputs_len,
        },
        &kernel,
        ptr_ty,
        num_bufs,
        [grid_x, grid_y, grid_z],
        [block_x, block_y, block_z],
    );

    // Populate uniform bounds (loop end) and runtime range starts if present.
    // `uniform_bound_present` packs both: bits 0..3 = bound x/y/z present,
    // bits 3..6 = start x/y/z present.
    let zero_i64 = builder.ins().iconst(cl_types::I64, 0);
    let mut present = 0u64;

    for (op, offset, bit) in [
        (uniform_bound_x, desc_layout::UNIFORM_BOUND_X_VALUE, 1u64),
        (uniform_bound_y, desc_layout::UNIFORM_BOUND_Y_VALUE, 2u64),
        (uniform_bound_z, desc_layout::UNIFORM_BOUND_Z_VALUE, 4u64),
        (uniform_start_x, desc_layout::UNIFORM_START_X_VALUE, 8u64),
        (uniform_start_y, desc_layout::UNIFORM_START_Y_VALUE, 16u64),
        (uniform_start_z, desc_layout::UNIFORM_START_Z_VALUE, 32u64),
    ] {
        let value = if let Some(op) = op {
            present |= bit;
            read_bound_operand(builder, op, locals, type_ctx)?
        } else {
            zero_i64
        };
        builder
            .ins()
            .store(MemFlags::new(), value, slots.desc_addr, offset);
    }

    let present_i64 = builder.ins().iconst(cl_types::I64, present as i64);
    builder.ins().store(
        MemFlags::new(),
        present_i64,
        slots.desc_addr,
        desc_layout::UNIFORM_BOUND_PRESENT,
    );

    let func_id = declare_launch_fn(module_ctx.module, ptr_ty)?;
    let local_func = module_ctx
        .module
        .declare_func_in_func(func_id, builder.func);
    let call = builder.ins().call(local_func, &[slots.desc_addr]);

    exit_on_launch_failure(builder, module_ctx.module, call)?;

    Ok(())
}

/// Ends the program cleanly when the launch entry returns failure (`0`).
///
/// The GPU runtime writes the reason to stderr before returning `0`; the
/// failure branch then calls the core runtime's noreturn launch-failure
/// helper, which reports MER_RT_013 and exits with status 1, so a refused
/// launch never dies on a signal. The trap after the call only terminates the
/// block for the verifier; it is never reached.
///
/// After this call, the builder is positioned on the continuation block
/// (success path) so the parent `translate` function can proceed normally.
fn exit_on_launch_failure(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    call: cranelift_codegen::ir::Inst,
) -> Result<(), CodegenError> {
    let call_result = builder.inst_results(call)[0];
    let zero_i8 = builder.ins().iconst(cl_types::I8, 0);
    let failed = builder.ins().icmp(IntCC::Equal, call_result, zero_i8);

    let fail_block = builder.create_block();
    let cont_block = builder.create_block();
    builder.ins().brif(failed, fail_block, &[], cont_block, &[]);

    builder.switch_to_block(fail_block);
    let report = declare_launch_failure_fn(module)?;
    let report_ref = module.declare_func_in_func(report, builder.func);
    builder.ins().call(report_ref, &[]);
    builder.ins().trap(TrapCode::unwrap_user(1));
    builder.seal_block(fail_block);

    builder.switch_to_block(cont_block);
    builder.seal_block(cont_block);

    Ok(())
}

fn declare_launch_failure_fn(module: &mut ObjectModule) -> Result<FuncId, CodegenError> {
    let sig = module.make_signature();
    module
        .declare_function(rt::GPU_LAUNCH_FAILED_PANIC, Linkage::Import, &sig)
        .map_err(|err| {
            CodegenError::declare_function(rt::GPU_LAUNCH_FAILED_PANIC.to_string(), err.to_string())
        })
}

/// Writes `bytes` to a fresh stack array and returns its address, or a null
/// pointer when there are none (the runtime reads null as "all zero").
fn store_byte_array(builder: &mut FunctionBuilder, ptr_ty: cl_types::Type, bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return builder.ins().iconst(ptr_ty, 0);
    }
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        bytes.len() as u32,
        1,
    ));
    let addr = builder.ins().stack_addr(ptr_ty, slot, 0);
    for (offset, &byte) in bytes.iter().enumerate() {
        let value = builder.ins().iconst(cl_types::I8, i64::from(byte));
        builder
            .ins()
            .store(MemFlags::new(), value, addr, offset as i32);
    }
    addr
}

/// The runtime conversion code of every captured buffer, from the GPU wire
/// format of its element type. Lowering records whether each buffer needs a
/// conversion from the same rule; a buffer on which the two disagree was
/// typed differently at the two stages, and is refused rather than marshalled
/// at a width the kernel does not declare.
fn buffer_conversion_codes(
    args: &[Operand],
    needs_conversion: &[bool],
    type_ctx: &TypeCtx,
) -> Result<Vec<u8>, CodegenError> {
    args.iter()
        .zip(needs_conversion)
        .map(|(arg, &needs_conversion)| {
            let conversion = buffer_conversion(&operand_type(arg, type_ctx)?.kind);
            if conversion.is_identity() == needs_conversion {
                return Err(CodegenError::Internal(format!(
                    "GpuLaunch buffer {:?}: lowering and codegen disagree on its element conversion ({:?})",
                    arg, conversion
                )));
            }
            Ok(conversion.code())
        })
        .collect()
}

/// The declared type of a projection-free launch operand's local.
fn operand_type<'a>(op: &Operand, type_ctx: &'a TypeCtx) -> Result<&'a Type, CodegenError> {
    let (Operand::Copy(place) | Operand::Move(place)) = op else {
        return Err(CodegenError::Internal(
            "GpuLaunch operand must be a Copy/Move of a local".to_string(),
        ));
    };
    type_ctx
        .local_types
        .get(place.local.0)
        .copied()
        .ok_or_else(|| CodegenError::Internal(format!("unknown GpuLaunch local {:?}", place.local)))
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
    wire_conversion_addr: Value,
    scalar_inputs_addr: Value,
    scalar_inputs_len: Value,
}

/// Packs scalar captures into a binary blob on the stack, one 32-bit lane per
/// capture in capture order, each converted to its lane by the GPU wire
/// format ([`scalar_capture_wire`]).
fn populate_scalar_inputs(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    scalar_args: &[Operand],
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<(Value, Value), CodegenError> {
    let ptr_ty = type_ctx.ptr_type;
    if scalar_args.is_empty() {
        return Ok((
            builder.ins().iconst(ptr_ty, 0),
            builder.ins().iconst(cl_types::I64, 0),
        ));
    }

    let byte_size = (scalar_args.len() as u32) * SCALAR_LANE_BYTES;
    let slot = builder.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        byte_size,
        4,
    ));
    let addr = builder.ins().stack_addr(ptr_ty, slot, 0);

    for (index, op) in scalar_args.iter().enumerate() {
        let capture = read_scalar_capture(builder, op, index, locals, type_ctx)?;
        let lane = convert_scalar_to_lane(builder, module, capture)?;
        let offset = (index as u32 * SCALAR_LANE_BYTES) as i32;
        builder.ins().store(MemFlags::new(), lane, addr, offset);
    }

    Ok((addr, builder.ins().iconst(cl_types::I64, byte_size as i64)))
}

/// Reads the captured scalar `op` (the `index`-th capture) and pairs its value
/// with the conversion the GPU wire format assigns its type.
fn read_scalar_capture(
    builder: &mut FunctionBuilder,
    op: &Operand,
    index: usize,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<ScalarCapture, CodegenError> {
    let value = read_operand_value(builder, op, locals, type_ctx)?;
    let local_ty = operand_type(op, type_ctx)?;
    let wire = scalar_capture_wire(&local_ty.kind).ok_or_else(|| {
        CodegenError::Internal(format!(
            "unsupported scalar capture type in codegen: {:?}",
            local_ty.kind
        ))
    })?;
    Ok(ScalarCapture {
        value,
        conversion: wire.conversion,
        index,
    })
}

/// Bytes each captured scalar occupies in the `_Inputs` uniform block.
const SCALAR_LANE_BYTES: u32 = 4;

/// One captured scalar on its way into the uniform block: its host value, the
/// conversion to its lane, and its position (named in a range error).
#[derive(Clone, Copy)]
struct ScalarCapture {
    value: Value,
    conversion: WireConversion,
    index: usize,
}

/// Converts a captured scalar's host value to its 32-bit device lane. A 64-bit
/// integer is range-checked first: a value the lane cannot hold stops the
/// program with a runtime error instead of reaching the kernel truncated.
fn convert_scalar_to_lane(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    capture: ScalarCapture,
) -> Result<Value, CodegenError> {
    let value = capture.value;
    let lane = match capture.conversion {
        WireConversion::Identity => value,
        WireConversion::NarrowI64 | WireConversion::NarrowU64 => {
            guard_capture_range(builder, module, capture)?;
            resize_int_to_i32(builder, value, IntExtension::Signed)
        }
        WireConversion::WidenI8 | WireConversion::WidenI16 => {
            resize_int_to_i32(builder, value, IntExtension::Signed)
        }
        WireConversion::WidenU8 | WireConversion::WidenU16 => {
            resize_int_to_i32(builder, value, IntExtension::Unsigned)
        }
        WireConversion::DemoteF64 if builder.func.dfg.value_type(value) == cl_types::F64 => {
            builder.ins().fdemote(cl_types::F32, value)
        }
        WireConversion::DemoteF64 => value,
    };
    Ok(lane)
}

/// How a narrower integer fills the upper bits of a wider lane.
#[derive(Clone, Copy)]
enum IntExtension {
    Signed,
    Unsigned,
}

/// Resizes an integer value to `i32`: reduces a wider one, extends a narrower
/// one with `extension`, and leaves an `i32` unchanged.
fn resize_int_to_i32(
    builder: &mut FunctionBuilder,
    value: Value,
    extension: IntExtension,
) -> Value {
    let width = builder.func.dfg.value_type(value).bits();
    match (width.cmp(&32), extension) {
        (std::cmp::Ordering::Greater, _) => builder.ins().ireduce(cl_types::I32, value),
        (std::cmp::Ordering::Less, IntExtension::Signed) => {
            builder.ins().sextend(cl_types::I32, value)
        }
        (std::cmp::Ordering::Less, IntExtension::Unsigned) => {
            builder.ins().uextend(cl_types::I32, value)
        }
        (std::cmp::Ordering::Equal, _) => value,
    }
}

/// Emits the range check for a 64-bit capture narrowed into a 32-bit lane: the
/// value survives exactly when truncating it to the lane and extending it back
/// (signed for `i32`, unsigned for `u32`) reproduces it. On failure the GPU
/// runtime reports the value and the program leaves through the same
/// noreturn launch-failure exit a refused launch takes.
fn guard_capture_range(
    builder: &mut FunctionBuilder,
    module: &mut ObjectModule,
    capture: ScalarCapture,
) -> Result<(), CodegenError> {
    let value = capture.value;
    let value_ty = builder.func.dfg.value_type(value);
    let lane = builder.ins().ireduce(cl_types::I32, value);
    let round_trip = if capture.conversion == WireConversion::NarrowU64 {
        builder.ins().uextend(value_ty, lane)
    } else {
        builder.ins().sextend(value_ty, lane)
    };
    let fits = builder.ins().icmp(IntCC::Equal, round_trip, value);

    let fail_block = builder.create_block();
    let cont_block = builder.create_block();
    builder.ins().brif(fits, cont_block, &[], fail_block, &[]);

    builder.switch_to_block(fail_block);
    builder.seal_block(fail_block);
    let report = declare_capture_range_report_fn(module)?;
    let report_ref = module.declare_func_in_func(report, builder.func);
    let index = builder.ins().iconst(cl_types::I64, capture.index as i64);
    let code = builder
        .ins()
        .iconst(cl_types::I8, i64::from(capture.conversion.code()));
    builder.ins().call(report_ref, &[index, value, code]);
    let exit = declare_launch_failure_fn(module)?;
    let exit_ref = module.declare_func_in_func(exit, builder.func);
    builder.ins().call(exit_ref, &[]);
    builder.ins().trap(TrapCode::unwrap_user(1));

    builder.switch_to_block(cont_block);
    builder.seal_block(cont_block);
    Ok(())
}

fn declare_capture_range_report_fn(module: &mut ObjectModule) -> Result<FuncId, CodegenError> {
    const NAME: &str = "miri_gpu_capture_out_of_range";
    let mut sig = module.make_signature();
    sig.params.push(AbiParam::new(cl_types::I64));
    sig.params.push(AbiParam::new(cl_types::I64));
    sig.params.push(AbiParam::new(cl_types::I8));
    module
        .declare_function(NAME, Linkage::Import, &sig)
        .map_err(|err| CodegenError::declare_function(NAME.to_string(), err.to_string()))
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
        wire_conversion_addr,
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
    store(wire_conversion_addr, desc_layout::BUF_WIRE_CONVERSION);
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

/// Reads a launch loop-bound / range-start operand as an `i64` value.
///
/// Unlike a captured buffer pointer, a bound may be a compile-time constant —
/// e.g. `forall i in 0..SIZE` where `const SIZE = 64 * 64` folds to a literal.
/// Such a constant is materialized directly; a runtime bound falls back to the
/// projection-free-local read shared with buffer operands.
fn read_bound_operand(
    builder: &mut FunctionBuilder,
    op: &Operand,
    locals: &HashMap<Local, cranelift_frontend::Variable>,
    type_ctx: &TypeCtx,
) -> Result<Value, CodegenError> {
    if let Operand::Constant(c) = op {
        return match &c.literal {
            Literal::Integer(value) => {
                Ok(builder.ins().iconst(cl_types::I64, value.to_i128() as i64))
            }
            other => Err(CodegenError::Internal(format!(
                "GpuLaunch loop bound must be an integer constant, got {:?}",
                other
            ))),
        };
    }
    read_operand_value(builder, op, locals, type_ctx)
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
        saturate_to_u32(builder, x, ty_x),
        saturate_to_u32(builder, y, ty_y),
        saturate_to_u32(builder, z, ty_z),
    ))
}

/// Narrows a `Dim3` component to the descriptor's 32-bit field, saturating
/// anything outside `0..=u32::MAX` to `u32::MAX`. Truncating instead would wrap
/// an oversized or negative dimension to an unrelated grid that may launch; a
/// saturated one is always refused by the runtime's device-limit check.
fn saturate_to_u32(builder: &mut FunctionBuilder, value: Value, from: cl_types::Type) -> Value {
    if from == cl_types::I32 {
        return value;
    }
    // Compared unsigned, a negative component is larger than `u32::MAX`.
    let ceiling = builder.ins().iconst(from, i64::from(u32::MAX));
    let clamped = builder.ins().umin(value, ceiling);
    builder.ins().ireduce(cl_types::I32, clamped)
}

#[cfg(test)]
mod tests {
    use super::desc_layout;

    #[test]
    fn gpu_launch_desc_size_matches_runtime() {
        assert_eq!(desc_layout::DESC_SIZE as usize, 176);
    }

    #[test]
    fn gpu_launch_desc_field_offsets_match_runtime() {
        // Mirror the offset assertions from runtime::gpu::launch::desc_layout_tests
        // to catch field reordering that preserves total size.
        assert_eq!(desc_layout::WGSL_PTR, 0);
        assert_eq!(desc_layout::WGSL_LEN, 8);
        assert_eq!(desc_layout::ENTRY_PTR, 16);
        assert_eq!(desc_layout::ENTRY_LEN, 24);
        assert_eq!(desc_layout::GRID_X, 32);
        assert_eq!(desc_layout::GRID_Y, 36);
        assert_eq!(desc_layout::GRID_Z, 40);
        assert_eq!(desc_layout::BLOCK_X, 44);
        assert_eq!(desc_layout::BLOCK_Y, 48);
        assert_eq!(desc_layout::BLOCK_Z, 52);
        assert_eq!(desc_layout::NUM_BUFS, 56);
        assert_eq!(desc_layout::BUF_DATA_PTRS, 64);
        assert_eq!(desc_layout::BUF_BYTE_LENS, 72);
        assert_eq!(desc_layout::BUF_HANDLE_IDS, 80);
        assert_eq!(desc_layout::BUF_READ_ONLY, 88);
        assert_eq!(desc_layout::BUF_WIRE_CONVERSION, 96);
        assert_eq!(desc_layout::UNIFORM_BOUND_PRESENT, 104);
        assert_eq!(desc_layout::UNIFORM_BOUND_X_VALUE, 112);
        assert_eq!(desc_layout::UNIFORM_BOUND_Y_VALUE, 120);
        assert_eq!(desc_layout::UNIFORM_BOUND_Z_VALUE, 128);
        assert_eq!(desc_layout::SCALAR_INPUTS_PTR, 136);
        assert_eq!(desc_layout::SCALAR_INPUTS_LEN, 144);
        assert_eq!(desc_layout::UNIFORM_START_X_VALUE, 152);
        assert_eq!(desc_layout::UNIFORM_START_Y_VALUE, 160);
        assert_eq!(desc_layout::UNIFORM_START_Z_VALUE, 168);
    }
}
