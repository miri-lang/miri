// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR → WGSL text emitter.

use crate::ast::expression::ExpressionKind;
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::codegen::wgsl::types::{
    buffer_element, buffer_element_typename, scalar, vector_swizzle, vector_type, WgslScalar,
};
use crate::error::CodegenError;
use crate::mir::backend::BackendMetadata;
use crate::mir::{
    BinOp, Body, Constant, Dimension, GpuIntrinsic, Local, MathIntrinsic, Operand, Place,
    PlaceElem, Rvalue, StatementKind, StorageClass, TerminatorKind, UnOp,
};
use std::fmt::Write;

pub(super) struct Emitter {
    output: String,
}

impl Emitter {
    pub(super) fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    pub(super) fn finish(self) -> String {
        self.output
    }

    pub(super) fn emit_helper(&mut self, name: &str, body: &Body) -> Result<(), CodegenError> {
        self.emit_helper_fn(name, body)
    }

    pub(super) fn emit_kernel(
        &mut self,
        name: &str,
        body: &Body,
        default_workgroup_size: [u32; 3],
    ) -> Result<(), CodegenError> {
        let mut bindings = collect_buffer_bindings(body)?;

        // For GPU kernels, wrap atomic<T> element types if the original declaration
        // has Atomic<T> elements. This allows atomicAdd, atomicSub, etc. to work.
        for binding in &mut bindings {
            if is_atomic_buffer_element(body, binding.param_local) {
                binding.element_typename = Some(format!("atomic<{}>", binding.element_type.name()));
                binding.read_write = true;
            }
        }

        self.emit_bindings(&bindings)?;
        let workgroup_size = resolve_workgroup_size(body, default_workgroup_size);
        self.emit_entry_point(name, body, &bindings, workgroup_size)
    }

    fn emit_bindings(&mut self, bindings: &[BufferBinding]) -> Result<(), CodegenError> {
        let scalar_bindings: Vec<_> = bindings
            .iter()
            .filter(|b| b.scalar_field.is_some())
            .collect();

        if !scalar_bindings.is_empty() {
            writeln!(self.output, "struct _Inputs {{").map_err(emit_err)?;
            for binding in &scalar_bindings {
                if let Some(field) = &binding.scalar_field {
                    // Bools are encoded as u32 in the struct (WGSL doesn't support bool literals in structs)
                    let wire_type =
                        if binding.element_type == crate::codegen::wgsl::types::WgslScalar::Bool {
                            "u32"
                        } else {
                            binding.element_type.name()
                        };
                    writeln!(self.output, "  {}: {},", field, wire_type).map_err(emit_err)?;
                }
            }
            writeln!(self.output, "}}").map_err(emit_err)?;
            writeln!(self.output).map_err(emit_err)?;
        }

        for binding in bindings {
            if binding.is_uniform {
                if binding.scalar_field.is_some() {
                    continue;
                }
                writeln!(
                    self.output,
                    "@group({}) @binding({}) var<uniform> {}: {};",
                    binding.group,
                    binding.index,
                    binding.var_name,
                    binding.element_type.name(),
                )
                .map_err(emit_err)?;
            } else {
                let access = if binding.read_write {
                    "storage, read_write"
                } else {
                    "storage, read"
                };
                writeln!(
                    self.output,
                    "@group({}) @binding({}) var<{}> {}: array<{}>;",
                    binding.group,
                    binding.index,
                    access,
                    binding.var_name,
                    binding
                        .element_typename
                        .as_deref()
                        .unwrap_or_else(|| binding.element_type.name()),
                )
                .map_err(emit_err)?;
            }
        }

        if !scalar_bindings.is_empty() {
            let index = scalar_bindings[0].index;
            writeln!(
                self.output,
                "@group(0) @binding({}) var<uniform> _inputs: _Inputs;",
                index
            )
            .map_err(emit_err)?;
        }

        if !bindings.is_empty() {
            writeln!(self.output).map_err(emit_err)?;
        }
        Ok(())
    }

    fn emit_entry_point(
        &mut self,
        name: &str,
        body: &Body,
        bindings: &[BufferBinding],
        workgroup_size: [u32; 3],
    ) -> Result<(), CodegenError> {
        writeln!(
            self.output,
            "@compute @workgroup_size({}, {}, {})",
            workgroup_size[0], workgroup_size[1], workgroup_size[2]
        )
        .map_err(emit_err)?;
        writeln!(
            self.output,
            "fn {}(@builtin(global_invocation_id) {}: vec3<u32>, @builtin(local_invocation_id) {}: vec3<u32>, @builtin(workgroup_id) {}: vec3<u32>, @builtin(num_workgroups) {}: vec3<u32>) {{",
            name,
            GLOBAL_ID,
            LOCAL_ID,
            WORKGROUP_ID,
            NUM_WORKGROUPS,
        )
        .map_err(emit_err)?;

        let mut ctx = BodyEmitter::new(body, bindings, workgroup_size, &mut self.output)?;
        ctx.emit_local_declarations()?;
        ctx.emit_blocks()?;

        writeln!(self.output, "}}").map_err(emit_err)?;
        writeln!(self.output).map_err(emit_err)
    }

    fn emit_helper_fn(&mut self, name: &str, body: &Body) -> Result<(), CodegenError> {
        if body.local_decls.is_empty() {
            return Err(CodegenError::Internal(
                "Helper function must have at least a return local".to_string(),
            ));
        }

        let return_type = scalar(&body.local_decls[0].ty.kind)?;

        write!(self.output, "fn {}(", name).map_err(emit_err)?;

        // Parameters are locals 1..=arg_count, named to match how the body
        // references them (`_1`, `_2`, ...). The implicit trailing `allocator`
        // param belongs to the CPU/Perceus ABI and has no GPU counterpart, so
        // it is skipped — GPU call sites never pass it.
        let mut emitted = 0;
        for i in 1..=body.arg_count {
            let local_decl = body.local_decls.get(i).ok_or_else(|| {
                CodegenError::Internal(format!(
                    "WGSL backend: helper function missing param local {}",
                    i
                ))
            })?;
            if local_decl.name.as_deref() == Some("allocator") {
                continue;
            }
            if emitted > 0 {
                write!(self.output, ", ").map_err(emit_err)?;
            }
            let param_type = scalar(&local_decl.ty.kind)?;
            write!(
                self.output,
                "{}: {}",
                local_name(Local(i)),
                param_type.name()
            )
            .map_err(emit_err)?;
            emitted += 1;
        }

        writeln!(self.output, ") -> {} {{", return_type.name()).map_err(emit_err)?;

        // Helpers carry no `@workgroup_size`; the value is unused for non-entry bodies.
        let mut ctx = BodyEmitter::new(body, &[], [1, 1, 1], &mut self.output)?;
        ctx.return_local = Some(Local(0));
        ctx.emit_local_declarations()?;
        ctx.emit_blocks()?;

        writeln!(self.output, "}}").map_err(emit_err)?;
        writeln!(self.output).map_err(emit_err)
    }
}

fn emit_err(err: std::fmt::Error) -> CodegenError {
    CodegenError::Emit(err.to_string())
}

const GLOBAL_ID: &str = "_global_id";
const LOCAL_ID: &str = "_local_id";
const WORKGROUP_ID: &str = "_workgroup_id";
const NUM_WORKGROUPS: &str = "_num_workgroups";

fn resolve_workgroup_size(body: &Body, fallback: [u32; 3]) -> [u32; 3] {
    match &body.backend_metadata {
        Some(BackendMetadata::Gpu(meta)) => meta.workgroup_size.unwrap_or(fallback),
        None => fallback,
    }
}

#[derive(Debug)]
struct BufferBinding {
    /// 1-based parameter local that this binding represents.
    param_local: Local,
    group: u32,
    index: u32,
    /// WGSL identifier used inside the entry point.
    var_name: String,
    element_type: WgslScalar,
    /// Full WGSL element-type spelling for the `array<...>` declaration when the
    /// element is not a plain scalar (e.g. `vec3<f32>`). `None` falls back to
    /// `element_type.name()`.
    element_typename: Option<String>,
    read_write: bool,
    is_uniform: bool,
    /// For scalar captures: the struct field name (e.g., "f0", "f1").
    /// None for storage buffers and loop bound uniforms.
    scalar_field: Option<String>,
}

/// Converts a scalar type kind to WGSL wire format: int→i32, bool→u32, f32→f32, float→f64.
fn scalar_type_to_wgsl(ty: &TypeKind) -> Result<WgslScalar, CodegenError> {
    match ty {
        TypeKind::Int => Ok(WgslScalar::I32),
        TypeKind::Boolean => Ok(WgslScalar::U32),
        TypeKind::F32 => Ok(WgslScalar::F32),
        TypeKind::Float | TypeKind::F64 => Ok(WgslScalar::F64),
        _ => Err(CodegenError::Internal(format!(
            "unsupported scalar capture type in WGSL backend: {:?}",
            ty
        ))),
    }
}

fn collect_buffer_bindings(body: &Body) -> Result<Vec<BufferBinding>, CodegenError> {
    let mut bindings = Vec::new();
    let mut binding_index = 0u32;

    // First pass: collect storage buffers.
    for param_idx in 1..=body.arg_count {
        let decl = body.local_decls.get(param_idx).ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: local_decls length {} <= param_idx {}",
                body.local_decls.len(),
                param_idx
            ))
        })?;

        // Only storage buffers in the first pass.
        if !matches!(
            decl.storage_class,
            StorageClass::GpuGlobal | StorageClass::StorageBuffer
        ) {
            continue;
        }

        let read_write = body.out_params.get(param_idx - 1).copied().ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: out_params length {} < arg_count {}",
                body.out_params.len(),
                body.arg_count
            ))
        })?;
        let element_type = buffer_element(&decl.ty.kind)?;
        let element_typename = buffer_element_typename(&decl.ty.kind)?;
        let var_name = decl
            .name
            .as_deref()
            .map(sanitize_identifier)
            .unwrap_or_else(|| format!("_buf{}", param_idx));
        bindings.push(BufferBinding {
            param_local: Local(param_idx),
            group: 0,
            index: binding_index,
            var_name,
            element_type,
            element_typename: Some(element_typename),
            read_write,
            is_uniform: false,
            scalar_field: None,
        });
        binding_index += 1;
    }

    // Second pass: collect uniform buffers (loop bounds and scalar captures).
    // Reserve one binding index for all pooled scalar fields (_Inputs struct).
    let mut inputs_binding: Option<u32> = None;
    let mut scalar_field_index = 0u32;
    for param_idx in 1..=body.arg_count {
        let decl = body.local_decls.get(param_idx).ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: local_decls length {} <= param_idx {}",
                body.local_decls.len(),
                param_idx
            ))
        })?;
        if decl.storage_class != StorageClass::UniformBuffer {
            continue;
        }

        let var_name = decl
            .name
            .as_deref()
            .map(sanitize_identifier)
            .unwrap_or_else(|| format!("_uniform{}", param_idx));

        let is_loop_bound =
            var_name.starts_with("_bound") || var_name.starts_with("_uniform_bound");

        if is_loop_bound {
            bindings.push(BufferBinding {
                param_local: Local(param_idx),
                group: 0,
                index: binding_index,
                var_name,
                element_type: WgslScalar::U32,
                element_typename: None,
                read_write: false,
                is_uniform: true,
                scalar_field: None,
            });
            binding_index += 1;
        } else {
            // First scalar field: reserve the binding index for the _Inputs struct
            if inputs_binding.is_none() {
                inputs_binding = Some(binding_index);
                binding_index += 1;
            }
            let scalar_field = format!("f{}", scalar_field_index);
            let element_type = scalar_type_to_wgsl(&decl.ty.kind)?;
            bindings.push(BufferBinding {
                param_local: Local(param_idx),
                group: 0,
                index: inputs_binding.unwrap(),
                var_name,
                element_type,
                element_typename: None,
                read_write: false,
                is_uniform: true,
                scalar_field: Some(scalar_field),
            });
            scalar_field_index += 1;
        }
    }

    // Validate all parameters.
    for param_idx in 1..=body.arg_count {
        let decl = body.local_decls.get(param_idx).ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: local_decls length {} <= param_idx {}",
                body.local_decls.len(),
                param_idx
            ))
        })?;
        match decl.storage_class {
            StorageClass::GpuGlobal | StorageClass::StorageBuffer | StorageClass::UniformBuffer => {
            }
            StorageClass::Stack
            | StorageClass::GpuShared
            | StorageClass::GpuConstant
            | StorageClass::GpuPrivate => {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: kernel parameter _{} has unsupported storage class {:?}; \
                     expected GpuGlobal/StorageBuffer/UniformBuffer",
                    param_idx, decl.storage_class
                )));
            }
        }
    }

    Ok(bindings)
}

/// Check if a kernel parameter is an Array of Atomic elements.
fn is_atomic_buffer_element(body: &Body, param_local: crate::mir::Local) -> bool {
    use crate::ast::expression::ExpressionKind;
    use crate::ast::types::BuiltinCollectionKind;

    let decl = match body.local_decls.get(param_local.0) {
        Some(d) => d,
        None => return false,
    };

    let elem_kind = match &decl.ty.kind {
        TypeKind::Custom(name, Some(args))
            if matches!(
                BuiltinCollectionKind::from_name(name),
                Some(BuiltinCollectionKind::Array) | Some(BuiltinCollectionKind::List)
            ) =>
        {
            match args.first() {
                Some(expr) => match &expr.node {
                    ExpressionKind::Type(ty, _) => &ty.kind,
                    _ => return false,
                },
                None => return false,
            }
        }
        _ => return false,
    };

    match elem_kind {
        TypeKind::Custom(name, Some(inner_args)) => {
            name == crate::ast::types::ATOMIC_TYPE_NAME && inner_args.len() == 1
        }
        _ => false,
    }
}

fn sanitize_identifier(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    s
}

/// Info about a loop header: exit block, body entry, and continue target.
#[derive(Debug, Clone)]
struct LoopInfo {
    /// The block jumped to when the loop condition is false (loop exit).
    exit: crate::mir::BasicBlock,
    /// The block where the loop body begins (after the header's SwitchInt).
    body_entry: crate::mir::BasicBlock,
    /// For for-loops: the continuing block (single latch != body_entry).
    /// For while-loops: None (header is the continue target).
    continuing: Option<crate::mir::BasicBlock>,
    /// The block to jump to on `continue` (either continuing block or header).
    continue_target: crate::mir::BasicBlock,
}

/// A frame on the loop stack, tracking where break/continue should jump.
#[derive(Debug, Clone)]
struct LoopFrame {
    exit: crate::mir::BasicBlock,
    continue_target: crate::mir::BasicBlock,
}

struct BodyEmitter<'a> {
    body: &'a Body,
    bindings: &'a [BufferBinding],
    workgroup_size: [u32; 3],
    output: &'a mut String,
    indent: usize,
    /// Blocks that are loop headers (targets of back-edges).
    loop_headers: std::collections::HashSet<crate::mir::BasicBlock>,
    /// Per-header loop info (exit, body_entry, continuing, continue_target).
    loop_info: std::collections::HashMap<crate::mir::BasicBlock, LoopInfo>,
    /// Stack of active loop frames for break/continue resolution.
    loop_stack: Vec<LoopFrame>,
    /// For a value-returning helper function, the local holding the return
    /// value (`_0`). `None` for `@compute` kernel entry points, which return
    /// `void` and read/write storage buffers instead.
    return_local: Option<Local>,
}

impl<'a> BodyEmitter<'a> {
    fn new(
        body: &'a Body,
        bindings: &'a [BufferBinding],
        workgroup_size: [u32; 3],
        output: &'a mut String,
    ) -> Result<Self, CodegenError> {
        let (loop_headers, loop_info, invalid_headers) = Self::detect_loops_and_build_info(body);

        // Reject if there are back-edges to non-SwitchInt blocks (invalid loop structure).
        if !invalid_headers.is_empty() {
            if let Some(bb) = invalid_headers.iter().min_by_key(|b| b.0) {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: back-edge to block bb{} without loop condition (SwitchInt); \
                     this is invalid loop structure and cannot be compiled to WGSL",
                    bb.0
                )));
            }
        }

        Ok(Self {
            body,
            bindings,
            workgroup_size,
            output,
            indent: 1,
            loop_headers,
            loop_info,
            loop_stack: Vec::new(),
            return_local: None,
        })
    }

    /// Detect loop headers and build per-header LoopInfo.
    /// Returns (valid_headers, loop_info, invalid_headers).
    /// Invalid headers are back-edges to blocks that are not proper SwitchInt loop headers.
    fn detect_loops_and_build_info(
        body: &Body,
    ) -> (
        std::collections::HashSet<crate::mir::BasicBlock>,
        std::collections::HashMap<crate::mir::BasicBlock, LoopInfo>,
        std::collections::HashSet<crate::mir::BasicBlock>,
    ) {
        let mut headers = std::collections::HashSet::new();
        let mut latches: std::collections::HashMap<
            crate::mir::BasicBlock,
            std::collections::HashSet<crate::mir::BasicBlock>,
        > = std::collections::HashMap::new();
        let mut visited = std::collections::HashSet::new();
        let mut on_stack = std::collections::HashSet::new();

        Self::dfs_find_back_edges(
            crate::mir::BasicBlock(0),
            body,
            &mut visited,
            &mut on_stack,
            &mut headers,
            &mut latches,
        );

        // Build LoopInfo for each header.
        let mut loop_info = std::collections::HashMap::new();
        for header in &headers {
            if let Some(header_block) = body.basic_blocks.get(header.0) {
                if let Some(term) = &header_block.terminator {
                    if let TerminatorKind::SwitchInt {
                        targets, otherwise, ..
                    } = &term.kind
                    {
                        if targets.len() == 1
                            && targets[0].0 == crate::mir::Discriminant::bool_true()
                        {
                            let body_entry = targets[0].1;
                            let exit = *otherwise;
                            let latch_set = latches.get(header).cloned().unwrap_or_default();

                            // Determine if this is a for-loop or while-loop.
                            let (continuing, continue_target) = if latch_set.len() == 1 {
                                if let Some(&latch) = latch_set.iter().next() {
                                    if latch == body_entry {
                                        // Single latch == body_entry => while-loop style.
                                        (None, *header)
                                    } else {
                                        // Single latch != body_entry => for-loop style.
                                        (Some(latch), latch)
                                    }
                                } else {
                                    // len() == 1 but iter().next() is None: impossible but fallback.
                                    (None, *header)
                                }
                            } else {
                                // Multiple or zero latches => while-loop style.
                                (None, *header)
                            };

                            loop_info.insert(
                                *header,
                                LoopInfo {
                                    exit,
                                    body_entry,
                                    continuing,
                                    continue_target,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Identify invalid headers: back-edges to blocks that are not proper SwitchInt loop headers.
        let mut invalid_headers = std::collections::HashSet::new();
        for header in &headers {
            if !loop_info.contains_key(header) {
                invalid_headers.insert(*header);
            }
        }

        // Remove invalid headers so they are not treated as loops.
        headers.retain(|h| loop_info.contains_key(h));

        (headers, loop_info, invalid_headers)
    }

    fn dfs_find_back_edges(
        bb: crate::mir::BasicBlock,
        body: &Body,
        visited: &mut std::collections::HashSet<crate::mir::BasicBlock>,
        on_stack: &mut std::collections::HashSet<crate::mir::BasicBlock>,
        headers: &mut std::collections::HashSet<crate::mir::BasicBlock>,
        latches: &mut std::collections::HashMap<
            crate::mir::BasicBlock,
            std::collections::HashSet<crate::mir::BasicBlock>,
        >,
    ) {
        if visited.contains(&bb) {
            return;
        }
        visited.insert(bb);
        on_stack.insert(bb);

        if let Some(block) = body.basic_blocks.get(bb.0) {
            if let Some(term) = &block.terminator {
                match &term.kind {
                    crate::mir::TerminatorKind::Goto { target } => {
                        if on_stack.contains(target) {
                            headers.insert(*target);
                            latches.entry(*target).or_default().insert(bb);
                        } else if !visited.contains(target) {
                            Self::dfs_find_back_edges(
                                *target, body, visited, on_stack, headers, latches,
                            );
                        }
                    }
                    crate::mir::TerminatorKind::SwitchInt {
                        targets, otherwise, ..
                    } => {
                        for (_, t) in targets {
                            if on_stack.contains(t) {
                                headers.insert(*t);
                                latches.entry(*t).or_default().insert(bb);
                            } else if !visited.contains(t) {
                                Self::dfs_find_back_edges(
                                    *t, body, visited, on_stack, headers, latches,
                                );
                            }
                        }
                        if on_stack.contains(otherwise) {
                            headers.insert(*otherwise);
                            latches.entry(*otherwise).or_default().insert(bb);
                        } else if !visited.contains(otherwise) {
                            Self::dfs_find_back_edges(
                                *otherwise, body, visited, on_stack, headers, latches,
                            );
                        }
                    }
                    crate::mir::TerminatorKind::Call { target, .. }
                    | crate::mir::TerminatorKind::VirtualCall { target, .. }
                    | crate::mir::TerminatorKind::GpuLaunch { target, .. } => {
                        if let Some(t) = target {
                            if on_stack.contains(t) {
                                headers.insert(*t);
                                latches.entry(*t).or_default().insert(bb);
                            } else if !visited.contains(t) {
                                Self::dfs_find_back_edges(
                                    *t, body, visited, on_stack, headers, latches,
                                );
                            }
                        }
                    }
                    crate::mir::TerminatorKind::Return
                    | crate::mir::TerminatorKind::Unreachable => {}
                }
            }
        }

        on_stack.remove(&bb);
    }

    fn write_indent(&mut self) -> Result<(), CodegenError> {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        Ok(())
    }

    fn emit_local_declarations(&mut self) -> Result<(), CodegenError> {
        // A value-returning helper accumulates its result in `_0`; declare it
        // up front so the body can assign to it before `return _0;`.
        if let Some(rl) = self.return_local {
            if let Some(decl) = self.body.local_decls.get(rl.0) {
                if !matches!(decl.ty.kind, TypeKind::Void) {
                    let ty_name = if let Some(vec_ty) = vector_type(&decl.ty.kind) {
                        vec_ty
                    } else {
                        scalar(&decl.ty.kind)?.name().to_string()
                    };
                    let zero_val = self.zero_init_value(&decl.ty.kind)?;
                    self.write_indent()?;
                    writeln!(
                        self.output,
                        "var {}: {} = {};",
                        local_name(rl),
                        ty_name,
                        zero_val
                    )
                    .map_err(emit_err)?;
                }
            }
        }
        let skip_until = self.body.arg_count + 1;
        for (i, decl) in self.body.local_decls.iter().enumerate() {
            if i == 0 || i < skip_until {
                continue;
            }
            if matches!(decl.ty.kind, TypeKind::Void) {
                continue;
            }
            let ty_name = if let Some(vec_ty) = vector_type(&decl.ty.kind) {
                vec_ty
            } else {
                scalar(&decl.ty.kind)?.name().to_string()
            };
            let zero_val = self.zero_init_value(&decl.ty.kind)?;
            self.write_indent()?;
            writeln!(
                self.output,
                "var {}: {} = {};",
                local_name(Local(i)),
                ty_name,
                zero_val
            )
            .map_err(emit_err)?;
        }
        Ok(())
    }

    fn binding_name(&self, local: Local) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| b.param_local == local)
            .map(|b| b.var_name.as_str())
    }

    fn get_scalar_field(&self, local: Local) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| b.param_local == local)
            .and_then(|b| b.scalar_field.as_deref())
    }

    fn emit_blocks(&mut self) -> Result<(), CodegenError> {
        let mut visited = std::collections::HashSet::new();
        self.emit_from(crate::mir::BasicBlock(0), None, &mut visited)
    }

    /// Emits MIR basic blocks starting at `start`, following `Goto` chains
    /// linearly, structurizing a `SwitchInt(cond, [(true, then)], otherwise=merge)`
    /// terminator into a WGSL `if` statement, and structurizing loops.
    /// Stops when reaching `stop` (if any) or a `Return`.
    fn emit_from(
        &mut self,
        start: crate::mir::BasicBlock,
        stop: Option<crate::mir::BasicBlock>,
        visited: &mut std::collections::HashSet<crate::mir::BasicBlock>,
    ) -> Result<(), CodegenError> {
        let mut cur = start;
        loop {
            if Some(cur) == stop {
                return Ok(());
            }

            // Check if we've visited this block before (back-edge or convergence).
            if visited.contains(&cur) {
                // If we've reached our stop block (e.g., if-merge or loop header), return.
                if Some(cur) == stop {
                    return Ok(());
                }
                // Back-edge: must be a loop header.
                if self.loop_headers.contains(&cur) {
                    self.emit_loop(cur, visited)?;
                    // After the loop, set cur to the exit and continue.
                    let exit = self
                        .loop_info
                        .get(&cur)
                        .ok_or_else(|| {
                            CodegenError::Internal(format!(
                                "WGSL backend: loop header bb{} missing LoopInfo",
                                cur.0
                            ))
                        })?
                        .exit;
                    cur = exit;
                    continue;
                } else {
                    // Visited non-header block: this is a convergence point (diamond).
                    // Return so the caller can continue from here.
                    return Ok(());
                }
            }
            visited.insert(cur);
            // If this is a loop header encountered via forward edge, emit it as a loop.
            if self.loop_headers.contains(&cur) {
                self.emit_loop(cur, visited)?;
                // After the loop, set cur to the exit and continue.
                let exit = self
                    .loop_info
                    .get(&cur)
                    .ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "WGSL backend: loop header bb{} missing LoopInfo",
                            cur.0
                        ))
                    })?
                    .exit;
                cur = exit;
                continue;
            }

            let block = self.body.basic_blocks.get(cur.0).ok_or_else(|| {
                CodegenError::Internal(format!("WGSL backend: block bb{} out of bounds", cur.0))
            })?;
            for stmt in &block.statements {
                self.emit_statement(&stmt.kind)?;
            }
            let term = block.terminator.as_ref().ok_or_else(|| {
                CodegenError::Internal(format!("WGSL backend: block bb{} has no terminator", cur.0))
            })?;
            match &term.kind {
                TerminatorKind::Return => {
                    if let Some(rl) = self.return_local {
                        // A value-returning helper always returns its `_0` slot.
                        self.write_indent()?;
                        writeln!(self.output, "return {};", local_name(rl)).map_err(emit_err)?;
                    } else if !self.loop_stack.is_empty() || self.indent > 1 {
                        // Early return inside a loop/if requires explicit `return;`.
                        self.write_indent()?;
                        writeln!(self.output, "return;").map_err(emit_err)?;
                    }
                    return Ok(());
                }
                TerminatorKind::Unreachable => {
                    self.write_indent()?;
                    writeln!(self.output, "// unreachable").map_err(emit_err)?;
                    return Ok(());
                }
                TerminatorKind::Goto { target } => {
                    // Resolve the Goto against the loop stack.
                    if let Some(frame) = self.loop_stack.last() {
                        if *target == frame.exit {
                            // Jump to loop exit => emit break.
                            self.write_indent()?;
                            writeln!(self.output, "break;").map_err(emit_err)?;
                            return Ok(());
                        }
                        if Some(*target) == stop {
                            // Jump to stop block (e.g., if-merge, loop continue target).
                            return Ok(());
                        }
                        if *target == frame.continue_target {
                            // Jump to continue target => emit continue.
                            self.write_indent()?;
                            writeln!(self.output, "continue;").map_err(emit_err)?;
                            return Ok(());
                        }
                    }
                    // Otherwise, continue at that target.
                    cur = *target;
                }
                TerminatorKind::SwitchInt {
                    discr,
                    targets,
                    otherwise,
                } => {
                    let true_target =
                        targets.len() == 1 && targets[0].0 == crate::mir::Discriminant::bool_true();
                    let false_target = targets.len() == 1
                        && targets[0].0 == crate::mir::Discriminant::bool_false();
                    if true_target || false_target {
                        let then_bb = targets[0].1;
                        let otherwise_bb = *otherwise;
                        // A `false`-target switch (short-circuit `or`) jumps to `then_bb`
                        // when the discriminant is false, so negate the condition to keep
                        // `then_bb` as the if-body. This mirrors the `true`-target `and`
                        // path, which falls through to `otherwise_bb` on false.
                        let raw_cond = self.render_operand(discr)?;
                        let cond_str = if true_target {
                            format!("bool({})", raw_cond)
                        } else {
                            format!("!bool({})", raw_cond)
                        };

                        // Decide plain-if vs if-else by checking forward reachability.
                        let then_reaches_otherwise = self.forward_reachable(then_bb, otherwise_bb);

                        if then_reaches_otherwise {
                            // Plain if: otherwise_bb is the merge point.
                            self.write_indent()?;
                            writeln!(self.output, "if ({}) {{", cond_str).map_err(emit_err)?;
                            self.indent += 1;
                            self.emit_from(then_bb, Some(otherwise_bb), visited)?;
                            self.indent -= 1;

                            self.write_indent()?;
                            writeln!(self.output, "}}").map_err(emit_err)?;

                            // Continue at the merge point (otherwise_bb).
                            cur = otherwise_bb;
                        } else {
                            // If-else: find the merge point of both branches.
                            let merge = self.find_merge(then_bb, otherwise_bb);

                            self.write_indent()?;
                            writeln!(self.output, "if ({}) {{", cond_str).map_err(emit_err)?;
                            self.indent += 1;
                            self.emit_from(then_bb, merge, visited)?;
                            self.indent -= 1;

                            self.write_indent()?;
                            writeln!(self.output, "}} else {{").map_err(emit_err)?;
                            self.indent += 1;
                            self.emit_from(otherwise_bb, merge, visited)?;
                            self.indent -= 1;

                            self.write_indent()?;
                            writeln!(self.output, "}}").map_err(emit_err)?;

                            // Continue at the merge point if it exists.
                            if let Some(merge_bb) = merge {
                                cur = merge_bb;
                            } else {
                                // Both branches return/diverge: end here.
                                return Ok(());
                            }
                        }
                    } else {
                        return Err(CodegenError::Internal(format!(
                            "WGSL backend: SwitchInt shape not supported (targets={:?})",
                            targets
                        )));
                    }
                }
                TerminatorKind::Call {
                    func,
                    args,
                    destination,
                    target,
                    ..
                } => {
                    self.emit_call(func, args, destination, target.as_ref())?;
                    if let Some(target) = target {
                        cur = *target;
                    } else {
                        return Ok(());
                    }
                }
                TerminatorKind::GpuLaunch { .. } | TerminatorKind::VirtualCall { .. } => {
                    return Err(CodegenError::Internal(format!(
                        "WGSL backend: terminator {:?} not yet supported",
                        term.kind
                    )));
                }
            }
        }
    }

    /// Check if `target` is forward-reachable from `source` without crossing loop back-edges.
    /// Returns true if a path exists from source to target following only forward edges.
    fn forward_reachable(
        &self,
        source: crate::mir::BasicBlock,
        target: crate::mir::BasicBlock,
    ) -> bool {
        if source == target {
            return true;
        }
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(source);
        visited.insert(source);

        while let Some(bb) = queue.pop_front() {
            if let Some(block) = self.body.basic_blocks.get(bb.0) {
                if let Some(term) = &block.terminator {
                    let successors = Self::terminator_successors(&term.kind);
                    for succ in successors {
                        if succ == target {
                            return true;
                        }
                        // Don't cross loop back-edges.
                        if !self.loop_headers.contains(&succ) && !visited.contains(&succ) {
                            visited.insert(succ);
                            queue.push_back(succ);
                        }
                    }
                }
            }
        }
        false
    }

    /// Find the nearest block reachable from BOTH `a` and `b` by forward edges.
    /// Returns None if no common reachable block exists (both paths diverge/return).
    fn find_merge(
        &self,
        a: crate::mir::BasicBlock,
        b: crate::mir::BasicBlock,
    ) -> Option<crate::mir::BasicBlock> {
        let mut visited_a = std::collections::HashSet::new();
        let mut queue_a = std::collections::VecDeque::new();
        queue_a.push_back(a);
        visited_a.insert(a);

        while let Some(bb) = queue_a.pop_front() {
            if self.forward_reachable(b, bb) {
                return Some(bb);
            }
            if let Some(block) = self.body.basic_blocks.get(bb.0) {
                if let Some(term) = &block.terminator {
                    let successors = Self::terminator_successors(&term.kind);
                    for succ in successors {
                        if !self.loop_headers.contains(&succ) && !visited_a.contains(&succ) {
                            visited_a.insert(succ);
                            queue_a.push_back(succ);
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract successor blocks from a terminator.
    fn terminator_successors(term: &TerminatorKind) -> Vec<crate::mir::BasicBlock> {
        match term {
            TerminatorKind::Return => vec![],
            TerminatorKind::Unreachable => vec![],
            TerminatorKind::Goto { target } => vec![*target],
            TerminatorKind::SwitchInt {
                targets, otherwise, ..
            } => {
                let mut succs = targets.iter().map(|(_, bb)| *bb).collect::<Vec<_>>();
                succs.push(*otherwise);
                succs
            }
            TerminatorKind::Call { target, .. }
            | TerminatorKind::GpuLaunch { target, .. }
            | TerminatorKind::VirtualCall { target, .. } => target.iter().copied().collect(),
        }
    }

    /// Emit a loop starting at `header`.
    fn emit_loop(
        &mut self,
        header: crate::mir::BasicBlock,
        visited: &mut std::collections::HashSet<crate::mir::BasicBlock>,
    ) -> Result<(), CodegenError> {
        let loop_info = self
            .loop_info
            .get(&header)
            .ok_or_else(|| {
                CodegenError::Internal(format!(
                    "WGSL backend: loop header bb{} missing LoopInfo",
                    header.0
                ))
            })?
            .clone();

        self.write_indent()?;
        writeln!(self.output, "loop {{").map_err(emit_err)?;
        self.indent += 1;

        // Push the loop frame.
        self.loop_stack.push(LoopFrame {
            exit: loop_info.exit,
            continue_target: loop_info.continue_target,
        });

        // Emit the condition check at the header.
        let header_block = self.body.basic_blocks.get(header.0).ok_or_else(|| {
            CodegenError::Internal(format!("WGSL backend: block bb{} out of bounds", header.0))
        })?;

        // Emit header block statements (compute the loop condition).
        for stmt in &header_block.statements {
            self.emit_statement(&stmt.kind)?;
        }

        if let Some(term) = &header_block.terminator {
            if let TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise: _,
            } = &term.kind
            {
                if targets.len() == 1 && targets[0].0 == crate::mir::Discriminant::bool_true() {
                    let cond_str = self.render_operand(discr)?;
                    self.write_indent()?;
                    writeln!(self.output, "if (!(bool({}))) {{ break; }}", cond_str)
                        .map_err(emit_err)?;
                } else {
                    return Err(CodegenError::Internal(format!(
                        "WGSL backend: loop header bb{} has unexpected terminator shape",
                        header.0
                    )));
                }
            } else {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: loop header bb{} is not a SwitchInt",
                    header.0
                )));
            }
        }

        // Emit the body. For a for-loop, stop at the continuing block (latch).
        // For a while-loop, stop at the header (back-edge).
        let body_stop = loop_info.continuing.or(Some(header));
        self.emit_from(loop_info.body_entry, body_stop, visited)?;

        // If this is a for-loop, emit the continuing block.
        if let Some(continuing) = loop_info.continuing {
            self.write_indent()?;
            writeln!(self.output, "continuing {{").map_err(emit_err)?;
            self.indent += 1;

            // Emit statements only from the continuing block, not the terminator (which is Goto header).
            if let Some(cont_block) = self.body.basic_blocks.get(continuing.0) {
                for stmt in &cont_block.statements {
                    self.emit_statement(&stmt.kind)?;
                }
            }

            self.indent -= 1;
            self.write_indent()?;
            writeln!(self.output, "}}").map_err(emit_err)?;
        }

        self.loop_stack.pop();

        self.indent -= 1;
        self.write_indent()?;
        writeln!(self.output, "}}").map_err(emit_err)?;

        Ok(())
    }

    /// Emit a function call: `_dest = func_name(args); goto target`.
    fn emit_call(
        &mut self,
        func: &Operand,
        args: &[Operand],
        destination: &Place,
        _target: Option<&crate::mir::BasicBlock>,
    ) -> Result<(), CodegenError> {
        let func_name = match func {
            Operand::Constant(c) => match &c.literal {
                crate::ast::literal::Literal::Identifier(name) => name.clone(),
                _ => {
                    return Err(CodegenError::Internal(
                        "WGSL backend: call with non-identifier func".to_string(),
                    ));
                }
            },
            _ => {
                return Err(CodegenError::Internal(
                    "WGSL backend: call with non-constant func".to_string(),
                ));
            }
        };

        self.write_indent()?;
        let dest_str = self.render_place(destination)?;

        // The implicit CPU/Perceus `allocator` argument has no GPU counterpart;
        // the helper signature drops the matching param, so drop the argument too.
        let arg_strs: Result<Vec<_>, _> = args
            .iter()
            .filter(|a| !self.is_allocator_operand(a))
            .map(|a| self.render_operand(a))
            .collect();
        let args_str = arg_strs?.join(", ");

        writeln!(self.output, "{} = {}({});", dest_str, func_name, args_str).map_err(emit_err)?;

        Ok(())
    }

    /// True when the operand reads the body's implicit `allocator` local, which
    /// is part of the CPU ABI and must not appear in a GPU call.
    fn is_allocator_operand(&self, op: &Operand) -> bool {
        let place = match op {
            Operand::Copy(p) | Operand::Move(p) => p,
            Operand::Constant(_) => return false,
        };
        if !place.projection.is_empty() {
            return false;
        }
        self.body
            .local_decls
            .get(place.local.0)
            .and_then(|d| d.name.as_deref())
            == Some("allocator")
    }

    /// Emit a statement.
    fn emit_statement(&mut self, kind: &StatementKind) -> Result<(), CodegenError> {
        match kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                self.write_indent()?;
                let rhs = self.render_rvalue(rvalue)?;
                let rhs = self.coerce_intrinsic_to_dest(place, rvalue, rhs);
                if self.is_atomic_buffer_element_write(place) {
                    // Wrap bare writes to atomic buffer elements with atomicStore
                    let rendered = self.render_place(place)?;
                    writeln!(self.output, "atomicStore(&{}, {});", rendered, rhs).map_err(emit_err)
                } else {
                    let lhs = self.render_place(place)?;
                    writeln!(self.output, "{} = {};", lhs, rhs).map_err(emit_err)
                }
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => Ok(()),
        }
    }

    /// Coerces a kernel dimension-intrinsic read to the destination scalar
    /// width. The WGSL thread/block builtins are `vec3<u32>`, but their MIR
    /// destination local is `Int` (i32), so a bare assignment is a width
    /// mismatch naga rejects. When the rvalue is a value-producing dim read and
    /// the destination is a non-projected `Int` local, wrap it in `i32(...)`.
    fn coerce_intrinsic_to_dest(&self, place: &Place, rvalue: &Rvalue, rhs: String) -> String {
        let is_dim_read = matches!(
            rvalue,
            Rvalue::GpuIntrinsic(
                GpuIntrinsic::ThreadIdx(_)
                    | GpuIntrinsic::BlockIdx(_)
                    | GpuIntrinsic::BlockDim(_)
                    | GpuIntrinsic::GridDim(_)
                    | GpuIntrinsic::GlobalIdx(_),
            )
        );
        if !is_dim_read || !place.projection.is_empty() {
            return rhs;
        }
        let dest_is_int = self
            .body
            .local_decls
            .get(place.local.0)
            .is_some_and(|decl| matches!(decl.ty.kind, TypeKind::Int));
        if dest_is_int {
            format!("i32({})", rhs)
        } else {
            rhs
        }
    }

    fn render_place(&self, place: &Place) -> Result<String, CodegenError> {
        let mut rendered = if let Some(field) = self.get_scalar_field(place.local) {
            let base = format!("_inputs.{}", field);
            // Wrap bool scalar field reads with bool(...) to coerce u32 → bool
            if place.projection.is_empty() {
                if let Some(decl) = self.body.local_decls.get(place.local.0) {
                    if matches!(decl.ty.kind, TypeKind::Boolean) {
                        format!("bool({})", base)
                    } else {
                        base
                    }
                } else {
                    base
                }
            } else {
                base
            }
        } else if let Some(name) = self.binding_name(place.local) {
            name.to_string()
        } else {
            local_name(place.local)
        };
        for elem in &place.projection {
            match elem {
                PlaceElem::Field(idx) => {
                    if let Some(decl) = self.body.local_decls.get(place.local.0) {
                        if let Some(swizzle) = vector_swizzle(&decl.ty.kind, *idx) {
                            write!(rendered, ".{}", swizzle).map_err(emit_err)?;
                        } else {
                            write!(rendered, ".{}", idx).map_err(emit_err)?;
                        }
                    } else {
                        write!(rendered, ".{}", idx).map_err(emit_err)?;
                    }
                }
                PlaceElem::Index(local) => {
                    // naga rejects indexing `array<T>` with an `i64` value — array indices must be `i32`/`u32`.
                    // Int maps to WGSL i32 (identity cast is safe); an I64 value >= 2^31 would silently
                    // truncate and wrap into an in-bounds index, aliasing a valid element, so saturate into
                    // the non-negative i32 range with clamp() first — an out-of-range index stays out-of-range
                    // and WGSL storage-array bounds behavior handles it harmlessly. Any other index type renders bare.
                    let index_kind = self.body.local_decls.get(local.0).map(|decl| &decl.ty.kind);
                    if matches!(index_kind, Some(TypeKind::I64)) {
                        write!(
                            rendered,
                            "[i32(clamp({}, 0, 2147483647))]",
                            local_name(*local)
                        )
                        .map_err(emit_err)?;
                    } else if matches!(index_kind, Some(TypeKind::Int)) {
                        write!(rendered, "[i32({})]", local_name(*local)).map_err(emit_err)?;
                    } else {
                        write!(rendered, "[{}]", local_name(*local)).map_err(emit_err)?;
                    }
                }
                PlaceElem::Deref => {
                    return Err(CodegenError::Internal(
                        "WGSL backend: PlaceElem::Deref not yet supported".into(),
                    ));
                }
            }
        }
        Ok(rendered)
    }

    fn render_operand(&self, op: &Operand) -> Result<String, CodegenError> {
        match op {
            Operand::Move(place) | Operand::Copy(place) => {
                let rendered = self.render_place(place)?;
                // Wrap bare reads of atomic buffer elements with atomicLoad
                if self.is_atomic_buffer_element_read(place) {
                    Ok(format!("atomicLoad(&{})", rendered))
                } else {
                    Ok(rendered)
                }
            }
            Operand::Constant(c) => render_constant(c),
        }
    }

    /// Check if a place refers to an indexed element of an atomic buffer.
    fn is_atomic_buffer_element(&self, place: &Place) -> bool {
        // Only apply to indexed buffer accesses
        if !place
            .projection
            .iter()
            .any(|e| matches!(e, PlaceElem::Index(_)))
        {
            return false;
        }

        let decl = match self.body.local_decls.get(place.local.0) {
            Some(d) => d,
            None => return false,
        };

        // Check if the buffer element type is Atomic
        match &decl.ty.kind {
            TypeKind::Custom(name, Some(args))
                if matches!(
                    crate::ast::BuiltinCollectionKind::from_name(name),
                    Some(crate::ast::BuiltinCollectionKind::Array)
                        | Some(crate::ast::BuiltinCollectionKind::List)
                ) =>
            {
                if let Some(elem_expr) = args.first() {
                    if let ExpressionKind::Type(elem_ty, _) = &elem_expr.node {
                        if let TypeKind::Custom(elem_name, Some(inner_args)) = &elem_ty.kind {
                            return elem_name == crate::ast::types::ATOMIC_TYPE_NAME
                                && !inner_args.is_empty();
                        }
                    }
                }
                false
            }
            _ => false,
        }
    }

    fn is_atomic_buffer_element_read(&self, place: &Place) -> bool {
        self.is_atomic_buffer_element(place)
    }

    fn is_atomic_buffer_element_write(&self, place: &Place) -> bool {
        self.is_atomic_buffer_element(place)
    }

    fn render_rvalue(&self, rvalue: &Rvalue) -> Result<String, CodegenError> {
        match rvalue {
            Rvalue::Use(op) => self.render_operand(op),
            Rvalue::BinaryOp(op, lhs, rhs) => {
                let lhs_str = self.render_operand(lhs)?;
                let rhs_str = self.render_operand(rhs)?;
                let sym = binop_symbol(*op)?;

                // Browser-portability: `Int` is now i32 in WGSL, so div/mod on int no longer
                // requires the i64 narrowing workaround. The workaround was needed only for
                // naga's MSL backend's i64 select ambiguity on Metal. Since `Int` → `i32`,
                // plain i32 div/mod is valid WGSL and naga handles it correctly.

                Ok(format!("({} {} {})", lhs_str, sym, rhs_str))
            }
            Rvalue::UnaryOp(op, val) => Ok(format!(
                "{}{}",
                unop_symbol(*op)?,
                self.render_operand(val)?
            )),
            Rvalue::Cast(op, ty) => Ok(format!(
                "{}({})",
                scalar(&ty.kind)?.name(),
                self.render_operand(op)?
            )),
            Rvalue::GpuIntrinsic(intrinsic) => Ok(self.render_gpu_intrinsic(*intrinsic)),
            Rvalue::MathIntrinsic(intrinsic, args) => {
                let rendered: Result<Vec<_>, _> =
                    args.iter().map(|a| self.render_operand(a)).collect();
                Ok(format!(
                    "{}({})",
                    math_intrinsic_name(*intrinsic),
                    rendered?.join(", ")
                ))
            }
            Rvalue::Aggregate(kind, operands) => self.render_aggregate(kind, operands),
            Rvalue::AtomicOp {
                op,
                buffer,
                index,
                value,
                compare_expected,
            } => self.render_atomic_op(*op, buffer, index, value, compare_expected.as_deref()),
            Rvalue::Len(_) | Rvalue::Ref(_) | Rvalue::Phi(_) | Rvalue::Allocate(_, _, _) => {
                Err(CodegenError::Internal(format!(
                    "WGSL backend: rvalue {:?} not yet supported",
                    rvalue
                )))
            }
        }
    }

    fn render_aggregate(
        &self,
        kind: &crate::mir::AggregateKind,
        operands: &[Operand],
    ) -> Result<String, CodegenError> {
        match kind {
            crate::mir::AggregateKind::Struct(ty) => {
                if let Some(vec_ty_name) = vector_type(&ty.kind) {
                    let rendered: Result<Vec<_>, _> =
                        operands.iter().map(|op| self.render_operand(op)).collect();
                    Ok(format!("{}({})", vec_ty_name, rendered?.join(", ")))
                } else {
                    Err(CodegenError::Internal(format!(
                        "WGSL backend: non-vector struct aggregate rvalue not yet supported: {}",
                        ty.kind
                    )))
                }
            }
            _ => Err(CodegenError::Internal(format!(
                "WGSL backend: rvalue aggregate kind {:?} not yet supported",
                kind
            ))),
        }
    }

    fn render_atomic_op(
        &self,
        op: crate::mir::backend::gpu::GpuAtomicOp,
        buffer: &Operand,
        index: &Operand,
        value: &Operand,
        compare_expected: Option<&Operand>,
    ) -> Result<String, CodegenError> {
        let buffer_str = self.render_operand(buffer)?;
        let index_str = self.render_operand(index)?;
        let value_str = self.render_operand(value)?;

        // Format: atomicAdd(&buf[i], v)
        let addr = format!("&{}[{}]", buffer_str, index_str);

        let op_name = match op {
            crate::mir::backend::gpu::GpuAtomicOp::Add => "atomicAdd",
            crate::mir::backend::gpu::GpuAtomicOp::Sub => "atomicSub",
            crate::mir::backend::gpu::GpuAtomicOp::And => "atomicAnd",
            crate::mir::backend::gpu::GpuAtomicOp::Or => "atomicOr",
            crate::mir::backend::gpu::GpuAtomicOp::Xor => "atomicXor",
            crate::mir::backend::gpu::GpuAtomicOp::Min => "atomicMin",
            crate::mir::backend::gpu::GpuAtomicOp::Max => "atomicMax",
            crate::mir::backend::gpu::GpuAtomicOp::Exchange => "atomicExchange",
            crate::mir::backend::gpu::GpuAtomicOp::CompareExchange => {
                let expected_str = compare_expected
                    .ok_or_else(|| {
                        CodegenError::Internal(
                            "compare_exchange requires an expected value".to_string(),
                        )
                    })
                    .and_then(|e| self.render_operand(e))?;
                return Ok(format!(
                    "atomicCompareExchangeWeak({}, {}, {}).old_value",
                    addr, expected_str, value_str
                ));
            }
        };

        Ok(format!("{}({}, {})", op_name, addr, value_str))
    }

    fn zero_init_value(&self, kind: &TypeKind) -> Result<String, CodegenError> {
        if let Some(vec_ty_name) = vector_type(kind) {
            let vec_name = if let TypeKind::Custom(name, _) = kind {
                name.as_str()
            } else {
                return Err(CodegenError::Internal(
                    "vector_type matched but failed to extract vector name".to_string(),
                ));
            };

            let dim = crate::ast::types::vec_dim(vec_name).ok_or_else(|| {
                CodegenError::Internal(format!(
                    "vector_type matched '{}' but vec_dim returned None",
                    vec_name
                ))
            })?;

            let args = if let TypeKind::Custom(_, Some(args)) = kind {
                args
            } else {
                return Err(CodegenError::Internal(
                    "vector_type matched but vector has no type arguments".to_string(),
                ));
            };

            let first_arg = args.first().ok_or_else(|| {
                CodegenError::Internal("vector type has empty type arguments".to_string())
            })?;

            let elem_ty = if let ExpressionKind::Type(ty, _) = &first_arg.node {
                ty
            } else {
                return Err(CodegenError::Internal(
                    "vector type argument is not a type expression".to_string(),
                ));
            };

            let elem_scalar = scalar(&elem_ty.kind)?;
            let zero_literal = match elem_scalar {
                crate::codegen::wgsl::types::WgslScalar::I32
                | crate::codegen::wgsl::types::WgslScalar::I64 => "0",
                crate::codegen::wgsl::types::WgslScalar::U32
                | crate::codegen::wgsl::types::WgslScalar::U64 => "0u",
                crate::codegen::wgsl::types::WgslScalar::F32
                | crate::codegen::wgsl::types::WgslScalar::F64 => "0.0",
                crate::codegen::wgsl::types::WgslScalar::Bool => "false",
            };

            let zero_list = vec![zero_literal; dim as usize].join(", ");
            Ok(format!("{}({})", vec_ty_name, zero_list))
        } else {
            let wgsl_scalar = scalar(kind)?;
            match wgsl_scalar {
                crate::codegen::wgsl::types::WgslScalar::I32
                | crate::codegen::wgsl::types::WgslScalar::I64 => Ok("0".to_string()),
                crate::codegen::wgsl::types::WgslScalar::U32
                | crate::codegen::wgsl::types::WgslScalar::U64 => Ok("0u".to_string()),
                crate::codegen::wgsl::types::WgslScalar::F32
                | crate::codegen::wgsl::types::WgslScalar::F64 => Ok("0.0".to_string()),
                crate::codegen::wgsl::types::WgslScalar::Bool => Ok("false".to_string()),
            }
        }
    }
}

fn local_name(local: Local) -> String {
    format!("_{}", local.0)
}

fn binop_symbol(op: BinOp) -> Result<&'static str, CodegenError> {
    Ok(match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::BitXor => "^",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::Eq => "==",
        BinOp::Lt => "<",
        BinOp::Le => "<=",
        BinOp::Ne => "!=",
        BinOp::Ge => ">=",
        BinOp::Gt => ">",
        BinOp::Offset => {
            return Err(CodegenError::Internal(
                "WGSL backend: pointer offset is not representable".into(),
            ));
        }
    })
}

fn unop_symbol(op: UnOp) -> Result<&'static str, CodegenError> {
    match op {
        UnOp::Not => Ok("!"),
        UnOp::Neg => Ok("-"),
        UnOp::Await => Err(CodegenError::Internal(
            "WGSL backend: await is not valid inside a GPU kernel".into(),
        )),
    }
}

fn math_intrinsic_name(intrinsic: MathIntrinsic) -> &'static str {
    match intrinsic {
        MathIntrinsic::Abs => "abs",
        MathIntrinsic::Min => "min",
        MathIntrinsic::Max => "max",
        MathIntrinsic::Pow => "pow",
        MathIntrinsic::Sqrt => "sqrt",
        MathIntrinsic::Floor => "floor",
        MathIntrinsic::Ceil => "ceil",
        MathIntrinsic::Round => "round",
        MathIntrinsic::Sin => "sin",
        MathIntrinsic::Cos => "cos",
        MathIntrinsic::Tan => "tan",
        MathIntrinsic::Ln => "log",
        MathIntrinsic::Exp => "exp",
        MathIntrinsic::Tanh => "tanh",
        MathIntrinsic::Exp2 => "exp2",
        MathIntrinsic::Log2 => "log2",
        MathIntrinsic::Atan2 => "atan2",
        MathIntrinsic::Fract => "fract",
        MathIntrinsic::Clamp => "clamp",
        MathIntrinsic::Mix => "mix",
        MathIntrinsic::Smoothstep => "smoothstep",
        MathIntrinsic::Step => "step",
        MathIntrinsic::Sign => "sign",
        MathIntrinsic::VecDot => "dot",
        MathIntrinsic::VecLength => "length",
        MathIntrinsic::VecNormalize => "normalize",
        MathIntrinsic::VecCross => "cross",
        MathIntrinsic::VecReflect => "reflect",
        MathIntrinsic::VecMix => "mix",
    }
}

impl BodyEmitter<'_> {
    fn render_gpu_intrinsic(&self, intrinsic: GpuIntrinsic) -> String {
        match intrinsic {
            GpuIntrinsic::ThreadIdx(dim) => format!("{}.{}", LOCAL_ID, dimension_field(dim)),
            GpuIntrinsic::BlockIdx(dim) => format!("{}.{}", WORKGROUP_ID, dimension_field(dim)),
            GpuIntrinsic::BlockDim(dim) => {
                // WGSL has no shader-visible `workgroup_size_*` builtin; the
                // `@workgroup_size` attribute is compile-time only. Substitute
                // the literal so the value is observable from the kernel body.
                format!("{}u", self.workgroup_size[dim as usize])
            }
            GpuIntrinsic::GridDim(dim) => format!("{}.{}", NUM_WORKGROUPS, dimension_field(dim)),
            GpuIntrinsic::GlobalIdx(dim) => format!("{}.{}", GLOBAL_ID, dimension_field(dim)),
            GpuIntrinsic::SyncThreads => "workgroupBarrier()".into(),
        }
    }
}

fn dimension_field(dim: Dimension) -> &'static str {
    match dim {
        Dimension::X => "x",
        Dimension::Y => "y",
        Dimension::Z => "z",
    }
}

fn render_constant(c: &Constant) -> Result<String, CodegenError> {
    match &c.literal {
        Literal::Integer(i) => Ok(render_integer(i, &c.ty.kind)),
        Literal::Float(f) => Ok(render_float(f, &c.ty.kind)),
        Literal::Boolean(b) => Ok(b.to_string()),
        Literal::None | Literal::String(_) | Literal::Identifier(_) | Literal::Regex(_) => {
            Err(CodegenError::Internal(format!(
                "WGSL backend: cannot embed literal {:?}",
                c.literal
            )))
        }
    }
}

/// WGSL integer-literal suffixes encode width and signedness — `u` for u32,
/// `li` for i64, `lu` for u64, bare for i32 — so the parser cannot widen
/// an `i32` literal into a storage element by mistake.
///
/// Browser-portability: `Int` (default int type) maps to i32 in WGSL,
/// so renders as bare (e.g., `123` not `123li`). Explicit `I64` still
/// uses `li` suffix (for CPU-only code). Fixed-width `I32` is bare.
fn render_integer(i: &IntegerLiteral, ty: &TypeKind) -> String {
    let value = i.to_i128();
    match ty {
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U128 => format!("{}u", value),
        TypeKind::U64 => format!("{}lu", value),
        TypeKind::Int => value.to_string(), // Browser-portable: bare i32 literal
        TypeKind::I64 => format!("{}li", value), // Explicit i64 uses li suffix
        TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I128
        | TypeKind::Float
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_) => value.to_string(),
    }
}

/// WGSL float-literal suffixes: bare → AbstractFloat (unifies to f32 unless
/// a context demands otherwise), `f` → f32, `lf` → f64. We tag based on the
/// resolved Miri type so a literal feeding an `f64` storage element keeps
/// its width through naga's type checker.
fn render_float(f: &FloatLiteral, ty: &TypeKind) -> String {
    let body = match f {
        FloatLiteral::F32(bits) => format!("{:?}", f32::from_bits(*bits)),
        FloatLiteral::F64(bits) => format!("{:?}", f64::from_bits(*bits)),
    };
    match ty {
        TypeKind::F32 => body,
        TypeKind::Float | TypeKind::F64 => format!("{}lf", body),
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Boolean
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::String
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_) => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::Type;
    use crate::error::syntax::Span;
    use crate::mir::LocalDecl;

    /// Test that an I64 index is rendered with clamp to saturate into i32 range.
    #[test]
    fn test_render_place_i64_index_clamps() {
        // Minimal MIR body with an I64 index local.
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);

        // Create a local with type I64 for use as an index.
        let i64_local = body.new_local(LocalDecl::new(
            Type::new(TypeKind::I64, Span::default()),
            Span::default(),
        ));

        // Create a target local (the array being indexed) with a simple type.
        let target_local = body.new_local(LocalDecl::new(
            Type::new(TypeKind::Int, Span::default()),
            Span::default(),
        ));

        // Create a place with an I64 index projection.
        let place = Place {
            local: target_local,
            projection: vec![PlaceElem::Index(i64_local)],
        };

        // Create a minimal BodyEmitter. Empty bindings and empty output buffer.
        let mut output = String::new();
        let emitter = BodyEmitter {
            body: &body,
            bindings: &[],
            workgroup_size: [256, 1, 1],
            output: &mut output,
            indent: 0,
            loop_headers: std::collections::HashSet::new(),
            loop_info: std::collections::HashMap::new(),
            loop_stack: Vec::new(),
            return_local: None,
        };

        // Render the place.
        let rendered = emitter.render_place(&place).expect("render_place failed");

        // Assert that the rendered string contains the structured clamp: [i32(clamp(..., 0, 2147483647))].
        // This pins the exact emission pattern so comments/TODOs can't accidentally pass.
        assert!(
            rendered.contains("[i32(clamp("),
            "Expected [i32(clamp( in rendered I64 index, got: {}",
            rendered
        );
        assert!(
            rendered.contains(", 0, 2147483647))]"),
            "Expected , 0, 2147483647))] in rendered I64 index, got: {}",
            rendered
        );
    }

    /// Test that an Int (i32) index is rendered WITHOUT clamp (identity).
    #[test]
    fn test_render_place_int_index_no_clamp() {
        // Minimal MIR body with an Int index local.
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);

        // Create a local with type Int (i32) for use as an index.
        let int_local = body.new_local(LocalDecl::new(
            Type::new(TypeKind::Int, Span::default()),
            Span::default(),
        ));

        // Create a target local (the array being indexed).
        let target_local = body.new_local(LocalDecl::new(
            Type::new(TypeKind::Int, Span::default()),
            Span::default(),
        ));

        // Create a place with an Int index projection.
        let place = Place {
            local: target_local,
            projection: vec![PlaceElem::Index(int_local)],
        };

        // Create a minimal BodyEmitter.
        let mut output = String::new();
        let emitter = BodyEmitter {
            body: &body,
            bindings: &[],
            workgroup_size: [256, 1, 1],
            output: &mut output,
            indent: 0,
            loop_headers: std::collections::HashSet::new(),
            loop_info: std::collections::HashMap::new(),
            loop_stack: Vec::new(),
            return_local: None,
        };

        // Render the place.
        let rendered = emitter.render_place(&place).expect("render_place failed");

        // Assert that the output has i32() but NOT clamp().
        assert!(
            rendered.contains("i32("),
            "Expected i32() in rendered Int index, got: {}",
            rendered
        );
        assert!(
            !rendered.contains("clamp("),
            "Unexpected clamp() in Int (i32) index (should be identity), got: {}",
            rendered
        );
    }

    /// Test that a non-Int/non-I64 index (e.g. F32) renders bare without i32() or clamp().
    #[test]
    fn test_render_place_other_index_renders_bare() {
        // Minimal MIR body with an F32 index local (a contrived case, but validates the fallback).
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);

        // Create a local with type F32 for use as an index.
        let f32_local = body.new_local(LocalDecl::new(
            Type::new(TypeKind::F32, Span::default()),
            Span::default(),
        ));

        // Create a target local (the array being indexed).
        let target_local = body.new_local(LocalDecl::new(
            Type::new(TypeKind::Int, Span::default()),
            Span::default(),
        ));

        // Create a place with an F32 index projection.
        let place = Place {
            local: target_local,
            projection: vec![PlaceElem::Index(f32_local)],
        };

        // Create a minimal BodyEmitter.
        let mut output = String::new();
        let emitter = BodyEmitter {
            body: &body,
            bindings: &[],
            workgroup_size: [256, 1, 1],
            output: &mut output,
            indent: 0,
            loop_headers: std::collections::HashSet::new(),
            loop_info: std::collections::HashMap::new(),
            loop_stack: Vec::new(),
            return_local: None,
        };

        // Render the place.
        let rendered = emitter.render_place(&place).expect("render_place failed");

        // Assert that the output is bare (no i32(), no clamp()).
        assert!(
            !rendered.contains("i32("),
            "Unexpected i32() cast in F32 index (should be bare), got: {}",
            rendered
        );
        assert!(
            !rendered.contains("clamp("),
            "Unexpected clamp() in F32 index (should be bare), got: {}",
            rendered
        );
        // Ensure the index is present in brackets.
        assert!(
            rendered.contains("["),
            "Expected [ in rendered place, got: {}",
            rendered
        );
    }
}
