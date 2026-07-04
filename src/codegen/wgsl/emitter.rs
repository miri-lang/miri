// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR → WGSL text emitter.

use crate::ast::expression::ExpressionKind;
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::codegen::wgsl::types::{
    buffer_element, buffer_element_typename, scalar, vector_swizzle, vector_type, WgslScalar,
};
use crate::codegen::wgsl::WgslSourceSpan;
use crate::error::syntax::Span;
use crate::error::CodegenError;
use crate::mir::backend::BackendMetadata;
use crate::mir::{
    BasicBlock, BinOp, Body, Constant, Dimension, GpuIndexNarrowing, GpuIntrinsic, Local,
    MathIntrinsic, Operand, Place, PlaceElem, Rvalue, StatementKind, StorageClass, TerminatorKind,
    UnOp, I32_INDEX_MAX,
};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt::Write;

pub(super) struct Emitter {
    output: String,
    /// WGSL-line → Miri-offset spans accumulated while emitting bodies.
    source_map: Vec<WgslSourceSpan>,
}

/// Lines the `enable f16;` preamble prepends to the module (`enable f16;` plus a
/// blank separator line). Source-map WGSL lines shift down by this when present.
const F16_PREAMBLE_LINES: u32 = 2;

impl Emitter {
    pub(super) fn new() -> Self {
        Self {
            output: String::new(),
            source_map: Vec::new(),
        }
    }

    /// WGSL requires `enable f16;` before any other global declaration when the
    /// module names the `f16` type. The substring is unambiguous: no other WGSL
    /// scalar spelling (`i32`/`u32`/`f32`/`f64`) contains it.
    fn needs_f16_preamble(&self) -> bool {
        self.output.contains("f16")
    }

    pub(super) fn finish(self) -> String {
        if self.needs_f16_preamble() {
            format!("enable f16;\n\n{}", self.output)
        } else {
            self.output
        }
    }

    /// Like [`finish`], but also returns the source map. The `enable f16;`
    /// preamble shifts every mapped WGSL line down, so entries are rebased when
    /// it is prepended.
    pub(super) fn finish_with_map(self) -> (String, Vec<WgslSourceSpan>) {
        if self.needs_f16_preamble() {
            let map = self
                .source_map
                .into_iter()
                .map(|s| WgslSourceSpan {
                    wgsl_line: s.wgsl_line + F16_PREAMBLE_LINES,
                    miri_offset: s.miri_offset,
                })
                .collect();
            (format!("enable f16;\n\n{}", self.output), map)
        } else {
            (self.output, self.source_map)
        }
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
        self.emit_shared_declarations(body)?;
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

    /// Emit module-scope `var<workgroup>` declarations for the kernel's `shared`
    /// arrays. WGSL requires workgroup variables at module scope (not inside the
    /// entry function) and forbids an initializer, so each shared array local is
    /// declared here and referenced by its source name inside the body.
    fn emit_shared_declarations(&mut self, body: &Body) -> Result<(), CodegenError> {
        let mut emitted_any = false;
        for (i, decl) in body.local_decls.iter().enumerate() {
            if decl.storage_class != StorageClass::GpuShared {
                continue;
            }
            let var_name = shared_local_name(decl, Local(i));
            let element = buffer_element_typename(&decl.ty.kind)?;
            let extent = shared_array_extent(&decl.ty.kind)?;
            writeln!(
                self.output,
                "var<workgroup> {}: array<{}, {}>;",
                var_name, element, extent
            )
            .map_err(emit_err)?;
            emitted_any = true;
        }
        if emitted_any {
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

        // Check if the kernel body uses warp intrinsics
        let (uses_warp_size, uses_lane_id) = scan_for_warp_intrinsics(body);

        // Build the function signature with warp builtins if needed
        let mut entry_sig = format!(
            "fn {}(@builtin(global_invocation_id) {}: vec3<u32>, @builtin(local_invocation_id) {}: vec3<u32>, @builtin(workgroup_id) {}: vec3<u32>, @builtin(num_workgroups) {}: vec3<u32>",
            name,
            GLOBAL_ID,
            LOCAL_ID,
            WORKGROUP_ID,
            NUM_WORKGROUPS,
        );

        if uses_warp_size {
            entry_sig.push_str(", @builtin(subgroup_size) SUBGROUP_SIZE: u32");
        }
        if uses_lane_id {
            entry_sig.push_str(", @builtin(subgroup_invocation_id) SUBGROUP_INVOCATION_ID: u32");
        }

        entry_sig.push_str(") {");
        writeln!(self.output, "{}", entry_sig).map_err(emit_err)?;

        let mut ctx = BodyEmitter::new(
            body,
            bindings,
            workgroup_size,
            &mut self.output,
            &mut self.source_map,
        )?;
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
        let mut ctx =
            BodyEmitter::new(body, &[], [1, 1, 1], &mut self.output, &mut self.source_map)?;
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

        // Loop-bound (`_bound_*`) and runtime range-start (`_start_*`) uniforms
        // are compiler-injected control scalars, each bound as its own `u32`
        // uniform rather than pooled into the `_Inputs` scalar-capture struct.
        let is_loop_bound = var_name.starts_with("_bound")
            || var_name.starts_with("_uniform_bound")
            || var_name.starts_with("_start");

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
    exit: BasicBlock,
    /// The block where the loop body begins (after the header's SwitchInt).
    body_entry: BasicBlock,
    /// For for-loops: the continuing block (single latch != body_entry).
    /// For while-loops: None (header is the continue target).
    continuing: Option<BasicBlock>,
    /// The block to jump to on `continue` (either continuing block or header).
    continue_target: BasicBlock,
}

/// A frame on the loop stack, tracking where break/continue should jump.
#[derive(Debug, Clone)]
struct LoopFrame {
    exit: BasicBlock,
    continue_target: BasicBlock,
}

/// The classification of a back-edge target block.
enum HeaderClass {
    /// A single-latch `SwitchInt` header that maps to a WGSL structured loop.
    Loop(LoopInfo),
    /// A valid `SwitchInt` header reached by more than one back-edge. WGSL's
    /// single continuing block cannot represent this without a `continue`
    /// skipping the loop increment, so it is rejected with a diagnostic.
    MultiLatch,
    /// A back-edge target that is not a `SwitchInt` boolean loop header at all.
    Invalid,
}

struct BodyEmitter<'a> {
    body: &'a Body,
    bindings: &'a [BufferBinding],
    workgroup_size: [u32; 3],
    output: &'a mut String,
    /// Accumulates WGSL-line → Miri-offset spans for the emitted body.
    source_map: &'a mut Vec<WgslSourceSpan>,
    /// Byte offset in `output` up to which newlines have already been counted,
    /// and the 1-based line of the next byte to be written. Together they let
    /// `current_line` advance in O(bytes-written) instead of rescanning.
    map_scan_pos: usize,
    map_line: u32,
    indent: usize,
    /// Blocks that are loop headers (targets of back-edges).
    loop_headers: HashSet<BasicBlock>,
    /// Per-block forward reachability, precomputed once. `reachability[b.0]` is
    /// the set of blocks reachable from `b` by forward edges without traversing
    /// *through* a loop header (a header may still be an endpoint). This turns
    /// the per-`if` `forward_reachable`/`find_merge` queries from O(blocks²) BFS
    /// into O(1) set lookups. Indexed by `BasicBlock.0`; empty for the
    /// render-place unit tests that never emit control flow.
    reachability: Vec<HashSet<BasicBlock>>,
    /// Per-header loop info (exit, body_entry, continuing, continue_target).
    loop_info: HashMap<BasicBlock, LoopInfo>,
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
        source_map: &'a mut Vec<WgslSourceSpan>,
    ) -> Result<Self, CodegenError> {
        let (loop_headers, loop_info, invalid_headers, multi_latch_headers) =
            Self::detect_loops_and_build_info(body);

        // Reject if there are back-edges to non-SwitchInt blocks (invalid loop structure).
        if !invalid_headers.is_empty() {
            if let Some(bb) = invalid_headers.iter().min_by_key(|b| b.0) {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: back-edge to block bb{} without loop condition (SwitchInt); \
                     WGSL only supports a structured loop whose header tests an exit condition \
                     (lowered to `loop {{ if (!cond) {{ break; }} ... }}`), so a condition-less \
                     or irreducible back-edge cannot be compiled to WGSL",
                    bb.0
                )));
            }
        }

        // Reject multi-latch loops: a header reached by more than one back-edge
        // cannot map to WGSL's single-`continuing`-block loop, since a `continue`
        // would then skip a for-loop's increment. Report the lowest header.
        if let Some(bb) = multi_latch_headers.iter().min_by_key(|b| b.0) {
            return Err(CodegenError::Internal(format!(
                "WGSL backend: loop header bb{} has multiple latches (back-edges); \
                 WGSL's single continuing block cannot represent this without a \
                 `continue` skipping the loop increment, so it cannot be compiled to WGSL",
                bb.0
            )));
        }

        let reachability = Self::compute_reachability(body, &loop_headers);

        Ok(Self {
            body,
            bindings,
            workgroup_size,
            output,
            source_map,
            map_scan_pos: 0,
            map_line: 1,
            indent: 1,
            loop_headers,
            reachability,
            loop_info,
            loop_stack: Vec::new(),
            return_local: None,
        })
    }

    /// Precompute, for every block, the set of blocks forward-reachable from it
    /// under the same rules `forward_reachable` used to walk on demand: follow
    /// terminator successors, never traverse *through* a loop header, but treat
    /// a header reached directly as a valid endpoint. Runs one BFS per block
    /// (O(blocks·(blocks+edges)) once) so each later `if`/merge query is an O(1)
    /// set lookup instead of an O(blocks²) walk.
    fn compute_reachability(
        body: &Body,
        loop_headers: &HashSet<BasicBlock>,
    ) -> Vec<HashSet<BasicBlock>> {
        (0..body.basic_blocks.len())
            .map(|i| Self::reachable_from(body, loop_headers, BasicBlock(i)))
            .collect()
    }

    /// The forward-reachability set of a single `source`. A block is included if
    /// it equals `source` or is a successor of any node reachable from `source`
    /// via non-loop-header intermediates — matching the old on-demand BFS exactly.
    fn reachable_from(
        body: &Body,
        loop_headers: &HashSet<BasicBlock>,
        source: BasicBlock,
    ) -> HashSet<BasicBlock> {
        let mut reachable = HashSet::new();
        reachable.insert(source);

        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(source);
        visited.insert(source);

        while let Some(bb) = queue.pop_front() {
            if let Some(block) = body.basic_blocks.get(bb.0) {
                if let Some(term) = &block.terminator {
                    for succ in Self::terminator_successors(&term.kind) {
                        reachable.insert(succ);
                        // Don't traverse through loop back-edges.
                        if !loop_headers.contains(&succ) && !visited.contains(&succ) {
                            visited.insert(succ);
                            queue.push_back(succ);
                        }
                    }
                }
            }
        }
        reachable
    }

    /// The 1-based WGSL line the next byte written to `output` will land on.
    /// Advances the newline scan only over bytes appended since the last call.
    fn current_line(&mut self) -> u32 {
        let added = self.output[self.map_scan_pos..]
            .bytes()
            .filter(|&b| b == b'\n')
            .count() as u32;
        self.map_line += added;
        self.map_scan_pos = self.output.len();
        self.map_line
    }

    /// Record that the construct about to be emitted on the current WGSL line
    /// originates at `span` in the Miri source. Synthetic spans (default, no
    /// source) are skipped. Because emission may add nothing, a later construct
    /// landing on the same line overwrites the entry — so only the construct
    /// that actually starts a line survives.
    fn record(&mut self, span: Span) {
        if span == Span::default() {
            return;
        }
        let wgsl_line = self.current_line();
        match self.source_map.last_mut() {
            Some(last) if last.wgsl_line == wgsl_line => last.miri_offset = span.start,
            _ => self.source_map.push(WgslSourceSpan {
                wgsl_line,
                miri_offset: span.start,
            }),
        }
    }

    /// Emit a statement, recording its source span first so the map ties the
    /// resulting WGSL line back to the Miri source.
    fn emit_statement_mapped(&mut self, stmt: &crate::mir::Statement) -> Result<(), CodegenError> {
        self.record(stmt.span);
        self.emit_statement(&stmt.kind)
    }

    /// Detect loop headers and build per-header LoopInfo.
    /// Returns (valid_headers, loop_info, invalid_headers, multi_latch_headers).
    /// Invalid headers are back-edges to blocks that are not proper SwitchInt
    /// loop headers. Multi-latch headers are valid SwitchInt loop headers reached
    /// by more than one back-edge — unstructurable for WGSL (see below).
    fn detect_loops_and_build_info(
        body: &Body,
    ) -> (
        HashSet<BasicBlock>,
        HashMap<BasicBlock, LoopInfo>,
        HashSet<BasicBlock>,
        HashSet<BasicBlock>,
    ) {
        let mut headers = HashSet::new();
        let mut latches: HashMap<BasicBlock, HashSet<BasicBlock>> = HashMap::new();
        let mut visited = HashSet::new();
        let mut on_stack = HashSet::new();

        Self::dfs_find_back_edges(
            BasicBlock(0),
            body,
            &mut visited,
            &mut on_stack,
            &mut headers,
            &mut latches,
        );

        // Classify each back-edge target as a structured loop, an unstructurable
        // multi-latch loop, or an invalid (non-SwitchInt) header.
        let mut loop_info = HashMap::new();
        let mut multi_latch_headers = HashSet::new();
        let mut invalid_headers = HashSet::new();
        for header in &headers {
            let latch_set = latches.get(header).cloned().unwrap_or_default();
            match Self::classify_loop_header(*header, body, &latch_set) {
                HeaderClass::Loop(info) => {
                    loop_info.insert(*header, info);
                }
                HeaderClass::MultiLatch => {
                    multi_latch_headers.insert(*header);
                }
                HeaderClass::Invalid => {
                    invalid_headers.insert(*header);
                }
            }
        }

        // Keep only compilable loop headers; invalid and multi-latch headers are
        // reported separately and are not treated as loops.
        headers.retain(|h| loop_info.contains_key(h));

        (headers, loop_info, invalid_headers, multi_latch_headers)
    }

    /// Classify a back-edge target block. A header is a structured loop only if
    /// its terminator is a `SwitchInt` with a single `bool_true` target (the body
    /// entry) and it has at most one latch. A header with more than one latch is
    /// unstructurable for WGSL: its single continuing block cannot carry a
    /// for-loop's increment for more than one back-edge, so a `continue` would
    /// skip that increment. (Miri's current lowering never emits this — a
    /// `continue` routes through the one increment latch — so the multi-latch arm
    /// is a defensive guard against a future lowering change.)
    fn classify_loop_header(
        header: BasicBlock,
        body: &Body,
        latch_set: &HashSet<BasicBlock>,
    ) -> HeaderClass {
        let Some(header_block) = body.basic_blocks.get(header.0) else {
            return HeaderClass::Invalid;
        };
        let Some(term) = &header_block.terminator else {
            return HeaderClass::Invalid;
        };
        let TerminatorKind::SwitchInt {
            targets, otherwise, ..
        } = &term.kind
        else {
            return HeaderClass::Invalid;
        };
        if targets.len() != 1 || targets[0].0 != crate::mir::Discriminant::bool_true() {
            return HeaderClass::Invalid;
        }

        let body_entry = targets[0].1;
        let exit = *otherwise;
        if latch_set.len() > 1 {
            return HeaderClass::MultiLatch;
        }

        // A single latch distinct from the body entry is a for-loop's increment
        // (the continuing block); otherwise (latch == body_entry, or the header
        // has no recorded latch) the loop is while-style and continues at the
        // header itself.
        let (continuing, continue_target) = match latch_set.iter().next() {
            Some(&latch) if latch != body_entry => (Some(latch), latch),
            _ => (None, header),
        };

        HeaderClass::Loop(LoopInfo {
            exit,
            body_entry,
            continuing,
            continue_target,
        })
    }

    /// Depth-first search for loop back-edges using an explicit worklist rather
    /// than recursion, so a pathologically deep CFG (long block chains) cannot
    /// overflow the call stack. A back-edge is an edge whose target is an
    /// ancestor still on the active DFS path (`on_stack`): its target is
    /// recorded as a loop header and its source as a latch of that header.
    ///
    /// Each `stack` frame carries a block, its ordered successors, and the index
    /// of the next successor to visit. Advancing a frame's index mirrors one
    /// iteration of the recursive successor loop; popping a frame mirrors the
    /// recursive return that clears the block from `on_stack`. Successors are
    /// taken from [`Terminator::successors`], preserving the exact
    /// Goto/SwitchInt(targets…, otherwise)/Call visit order the recursion used.
    fn dfs_find_back_edges(
        root: BasicBlock,
        body: &Body,
        visited: &mut HashSet<BasicBlock>,
        on_stack: &mut HashSet<BasicBlock>,
        headers: &mut HashSet<BasicBlock>,
        latches: &mut HashMap<BasicBlock, HashSet<BasicBlock>>,
    ) {
        if visited.contains(&root) {
            return;
        }

        let successors_of = |bb: BasicBlock| -> Vec<BasicBlock> {
            body.basic_blocks
                .get(bb.0)
                .and_then(|block| block.terminator.as_ref())
                .map(|term| term.successors())
                .unwrap_or_default()
        };

        visited.insert(root);
        on_stack.insert(root);
        let mut stack: Vec<(BasicBlock, Vec<BasicBlock>, usize)> =
            vec![(root, successors_of(root), 0)];

        while let Some(&(bb, _, _)) = stack.last() {
            let top = stack.len() - 1;
            let next = stack[top].2;
            if next < stack[top].1.len() {
                let target = stack[top].1[next];
                stack[top].2 += 1;
                if on_stack.contains(&target) {
                    // Back-edge: `target` is an active ancestor => loop header.
                    headers.insert(target);
                    latches.entry(target).or_default().insert(bb);
                } else if !visited.contains(&target) {
                    // Tree-edge: descend into `target` next (LIFO), so its whole
                    // subtree is explored and popped before `bb`'s remaining
                    // successors — identical to the recursive traversal.
                    visited.insert(target);
                    on_stack.insert(target);
                    let succs = successors_of(target);
                    stack.push((target, succs, 0));
                }
            } else {
                on_stack.remove(&bb);
                stack.pop();
            }
        }
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
            // `shared` arrays are declared at module scope as `var<workgroup>`,
            // never as a function-local `var`.
            if decl.storage_class == StorageClass::GpuShared {
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

    /// The module-scope `var<workgroup>` name for a `shared` array local, or
    /// `None` if the local is not shared. Matches the name emitted by
    /// [`Emitter::emit_shared_declarations`].
    fn shared_local_name(&self, local: Local) -> Option<String> {
        let decl = self.body.local_decls.get(local.0)?;
        if decl.storage_class != StorageClass::GpuShared {
            return None;
        }
        Some(shared_local_name(decl, local))
    }

    fn get_scalar_field(&self, local: Local) -> Option<&str> {
        self.bindings
            .iter()
            .find(|b| b.param_local == local)
            .and_then(|b| b.scalar_field.as_deref())
    }

    fn emit_blocks(&mut self) -> Result<(), CodegenError> {
        let mut visited = HashSet::new();
        self.emit_from(BasicBlock(0), None, &mut visited)
    }

    /// Emits MIR basic blocks starting at `start`, following `Goto` chains
    /// linearly, structurizing a `SwitchInt(cond, [(true, then)], otherwise=merge)`
    /// terminator into a WGSL `if` statement, and structurizing loops.
    /// Stops when reaching `stop` (if any) or a `Return`.
    fn emit_from(
        &mut self,
        start: BasicBlock,
        stop: Option<BasicBlock>,
        visited: &mut HashSet<BasicBlock>,
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
                    cur = self.loop_info_or_err(cur)?.exit;
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
                cur = self.loop_info_or_err(cur)?.exit;
                continue;
            }

            let block = self.body.basic_blocks.get(cur.0).ok_or_else(|| {
                CodegenError::Internal(format!("WGSL backend: block bb{} out of bounds", cur.0))
            })?;
            for stmt in &block.statements {
                self.emit_statement_mapped(stmt)?;
            }
            let term = block.terminator.as_ref().ok_or_else(|| {
                CodegenError::Internal(format!("WGSL backend: block bb{} has no terminator", cur.0))
            })?;
            // Map the control-flow line (an `if`, `return`, `break`/`continue`,
            // or call) back to the terminator's Miri source span.
            self.record(term.span);
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
    /// Backed by the reachability sets precomputed in [`Self::compute_reachability`];
    /// a `source` outside the block range (no precomputed set) is reachable only to itself.
    fn forward_reachable(&self, source: BasicBlock, target: BasicBlock) -> bool {
        match self.reachability.get(source.0) {
            Some(set) => set.contains(&target),
            None => source == target,
        }
    }

    /// Find the nearest block reachable from BOTH `a` and `b` by forward edges.
    /// Returns None if no common reachable block exists (both paths diverge/return).
    fn find_merge(&self, a: BasicBlock, b: BasicBlock) -> Option<BasicBlock> {
        let mut visited_a = HashSet::new();
        let mut queue_a = VecDeque::new();
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
    fn terminator_successors(term: &TerminatorKind) -> Vec<BasicBlock> {
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

    /// Look up the [`LoopInfo`] recorded for a loop `header`, erroring if none
    /// exists. Every block retained in `loop_headers` has a matching entry (both
    /// are populated together in `detect_loops_and_build_info`), so a miss can
    /// only mean an internal invariant break — reported with the header named.
    fn loop_info_or_err(&self, header: BasicBlock) -> Result<&LoopInfo, CodegenError> {
        self.loop_info.get(&header).ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: loop header bb{} missing LoopInfo",
                header.0
            ))
        })
    }

    /// Emit a loop starting at `header`.
    fn emit_loop(
        &mut self,
        header: BasicBlock,
        visited: &mut HashSet<BasicBlock>,
    ) -> Result<(), CodegenError> {
        let loop_info = self.loop_info_or_err(header)?.clone();

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
            self.emit_statement_mapped(stmt)?;
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
                    self.record(term.span);
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
                    self.emit_statement_mapped(stmt)?;
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
        _target: Option<&BasicBlock>,
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
                // A workgroup barrier is a void side-effecting intrinsic: emit the
                // bare statement form, never an assignment into its throwaway temp.
                if matches!(rvalue, Rvalue::GpuIntrinsic(GpuIntrinsic::SyncThreads)) {
                    self.write_indent()?;
                    return writeln!(self.output, "workgroupBarrier();").map_err(emit_err);
                }
                // A `void`-typed destination is a discarded statement result (e.g.
                // the temp an expression-statement materializes a void call into).
                // WGSL has no void values, so the assignment has nothing to emit.
                if self
                    .body
                    .local_decls
                    .get(place.local.0)
                    .is_some_and(|d| matches!(d.ty.kind, TypeKind::Void))
                {
                    return Ok(());
                }
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
                    | GpuIntrinsic::GlobalIdx(_)
                    | GpuIntrinsic::WarpSize
                    | GpuIntrinsic::LaneId,
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
        } else if let Some(name) = self.shared_local_name(place.local) {
            name
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
                    // naga rejects indexing `array<T>` with an `i64` value — array indices
                    // must be 32-bit. The narrowing policy (identity for `Int`, saturate for
                    // `I64`, none otherwise) is backend-neutral and lives in `mir::backend`;
                    // here we only render the chosen narrowing as WGSL. A saturated I64 index
                    // stays out-of-range when out-of-range, and WGSL storage-array bounds
                    // behavior handles it harmlessly.
                    let name = local_name(*local);
                    let narrowing = self
                        .body
                        .local_decls
                        .get(local.0)
                        .map(|decl| GpuIndexNarrowing::from_index_kind(&decl.ty.kind))
                        .unwrap_or(GpuIndexNarrowing::None);
                    match narrowing {
                        GpuIndexNarrowing::SaturateToI32 => {
                            write!(rendered, "[i32(clamp({}, 0, {}))]", name, I32_INDEX_MAX)
                                .map_err(emit_err)?
                        }
                        GpuIndexNarrowing::Identity => {
                            write!(rendered, "[i32({})]", name).map_err(emit_err)?
                        }
                        GpuIndexNarrowing::None => {
                            write!(rendered, "[{}]", name).map_err(emit_err)?
                        }
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
            Rvalue::GpuIntrinsic(intrinsic) => self.render_gpu_intrinsic(intrinsic.clone()),
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
                crate::codegen::wgsl::types::WgslScalar::F16
                | crate::codegen::wgsl::types::WgslScalar::F32
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
                crate::codegen::wgsl::types::WgslScalar::F16
                | crate::codegen::wgsl::types::WgslScalar::F32
                | crate::codegen::wgsl::types::WgslScalar::F64 => Ok("0.0".to_string()),
                crate::codegen::wgsl::types::WgslScalar::Bool => Ok("false".to_string()),
            }
        }
    }
}

fn local_name(local: Local) -> String {
    format!("_{}", local.0)
}

/// WGSL identifier for a `shared` (workgroup) array local: its sanitized source
/// name, falling back to a synthetic `_shared{n}` when the declaration is
/// anonymous. The same name is used at the module-scope declaration and at every
/// reference inside the kernel body.
fn shared_local_name(decl: &crate::mir::LocalDecl, local: Local) -> String {
    decl.name
        .as_deref()
        .map(sanitize_identifier)
        .unwrap_or_else(|| format!("_shared{}", local.0))
}

/// Const-evaluate the fixed extent `N` of a `shared` array's `Array<T, N>` type.
/// Workgroup arrays must be fixed-size, so a non-constant extent is a backend
/// error (the type checker requires a compile-time size for shared arrays).
fn shared_array_extent(kind: &TypeKind) -> Result<i128, CodegenError> {
    use crate::ast::types::BuiltinCollectionKind;
    let size_expr = match kind {
        TypeKind::Array(_, size) => size.as_ref(),
        TypeKind::Custom(name, Some(args))
            if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
                && args.len() == 2 =>
        {
            &args[1]
        }
        _ => {
            return Err(CodegenError::Internal(format!(
                "WGSL backend: shared variable type {:?} is not a fixed-size array",
                kind
            )))
        }
    };
    crate::type_checker::TypeChecker::try_eval_const_int(size_expr).ok_or_else(|| {
        CodegenError::Internal(
            "WGSL backend: shared array size must be a compile-time constant".to_string(),
        )
    })
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
    fn render_gpu_intrinsic(&self, intrinsic: GpuIntrinsic) -> Result<String, CodegenError> {
        match intrinsic {
            GpuIntrinsic::ThreadIdx(dim) => Ok(format!("{}.{}", LOCAL_ID, dimension_field(dim))),
            GpuIntrinsic::BlockIdx(dim) => Ok(format!("{}.{}", WORKGROUP_ID, dimension_field(dim))),
            GpuIntrinsic::BlockDim(dim) => {
                // WGSL has no shader-visible `workgroup_size_*` builtin; the
                // `@workgroup_size` attribute is compile-time only. Substitute
                // the literal so the value is observable from the kernel body.
                Ok(format!("{}u", self.workgroup_size[dim as usize]))
            }
            GpuIntrinsic::GridDim(dim) => {
                Ok(format!("{}.{}", NUM_WORKGROUPS, dimension_field(dim)))
            }
            GpuIntrinsic::GlobalIdx(dim) => Ok(format!("{}.{}", GLOBAL_ID, dimension_field(dim))),
            GpuIntrinsic::SyncThreads => Ok("workgroupBarrier()".into()),
            GpuIntrinsic::WarpSize => Ok("SUBGROUP_SIZE".into()),
            GpuIntrinsic::LaneId => Ok("SUBGROUP_INVOCATION_ID".into()),
            GpuIntrinsic::ShuffleDown(value_op, offset) => {
                let value_str = self.render_operand(value_op.as_ref())?;
                Ok(format!("subgroupShuffleDown({}, {}u)", value_str, offset))
            }
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
        | TypeKind::F16
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
        TypeKind::F16 => format!("{}h", body), // `h` suffix → f16 literal (needs `enable f16;`)
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

/// Scan the kernel body for uses of WarpSize and LaneId intrinsics.
/// Returns (uses_warp_size, uses_lane_id).
fn scan_for_warp_intrinsics(body: &Body) -> (bool, bool) {
    let mut uses_warp_size = false;
    let mut uses_lane_id = false;

    for block in &body.basic_blocks {
        for statement in &block.statements {
            if let StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) =
                &statement.kind
            {
                if let Rvalue::GpuIntrinsic(intrinsic) = rvalue {
                    match intrinsic {
                        GpuIntrinsic::WarpSize => uses_warp_size = true,
                        GpuIntrinsic::LaneId => uses_lane_id = true,
                        GpuIntrinsic::ShuffleDown(_, _) => {
                            // ShuffleDown doesn't need a builtin parameter
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    (uses_warp_size, uses_lane_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::Type;
    use crate::error::syntax::Span;
    use crate::mir::{LocalDecl, Terminator};

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
        let mut source_map = Vec::new();
        let emitter = BodyEmitter {
            body: &body,
            bindings: &[],
            workgroup_size: [256, 1, 1],
            output: &mut output,
            source_map: &mut source_map,
            map_scan_pos: 0,
            map_line: 1,
            indent: 0,
            loop_headers: HashSet::new(),
            reachability: Vec::new(),
            loop_info: HashMap::new(),
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
        let mut source_map = Vec::new();
        let emitter = BodyEmitter {
            body: &body,
            bindings: &[],
            workgroup_size: [256, 1, 1],
            output: &mut output,
            source_map: &mut source_map,
            map_scan_pos: 0,
            map_line: 1,
            indent: 0,
            loop_headers: HashSet::new(),
            reachability: Vec::new(),
            loop_info: HashMap::new(),
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
        let mut source_map = Vec::new();
        let emitter = BodyEmitter {
            body: &body,
            bindings: &[],
            workgroup_size: [256, 1, 1],
            output: &mut output,
            source_map: &mut source_map,
            map_scan_pos: 0,
            map_line: 1,
            indent: 0,
            loop_headers: HashSet::new(),
            reachability: Vec::new(),
            loop_info: HashMap::new(),
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

    /// Build a block with a `Goto` terminator to `target`.
    fn goto_block(target: BasicBlock) -> crate::mir::BasicBlockData {
        crate::mir::BasicBlockData::new(Some(Terminator::new(
            crate::mir::TerminatorKind::Goto { target },
            Span::default(),
        )))
    }

    /// Build a block whose terminator returns.
    fn return_block() -> crate::mir::BasicBlockData {
        crate::mir::BasicBlockData::new(Some(Terminator::new(
            crate::mir::TerminatorKind::Return,
            Span::default(),
        )))
    }

    /// A canonical for-loop header: `SwitchInt` on a dummy operand with a single
    /// `bool_true` target (the body entry) and `otherwise` as the loop exit.
    fn loop_header_block(body_entry: BasicBlock, exit: BasicBlock) -> crate::mir::BasicBlockData {
        crate::mir::BasicBlockData::new(Some(Terminator::new(
            crate::mir::TerminatorKind::SwitchInt {
                discr: Operand::Copy(Place::new(crate::mir::Local(0))),
                targets: vec![(crate::mir::Discriminant::bool_true(), body_entry)],
                otherwise: exit,
            },
            Span::default(),
        )))
    }

    /// A deep acyclic `Goto` chain must not overflow the call stack and must
    /// report no loop headers (a straight-line CFG has no back-edges). With the
    /// recursive detector this depth blew the thread stack; the explicit-worklist
    /// version handles it iteratively.
    #[test]
    fn test_back_edges_deep_chain_no_overflow() {
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        let depth = 100_000usize;
        for i in 0..depth {
            body.basic_blocks.push(goto_block(BasicBlock(i + 1)));
        }
        body.basic_blocks.push(return_block());

        let (headers, loop_info, invalid, multi_latch) =
            BodyEmitter::detect_loops_and_build_info(&body);

        assert!(
            headers.is_empty(),
            "acyclic chain must have no loop headers"
        );
        assert!(loop_info.is_empty(), "acyclic chain must have no LoopInfo");
        assert!(
            invalid.is_empty(),
            "acyclic chain must have no invalid headers"
        );
        assert!(
            multi_latch.is_empty(),
            "acyclic chain must have no multi-latch headers"
        );
    }

    /// A canonical single-latch for-loop is classified with the header, exit,
    /// body entry, and the increment block as the continuing/continue target.
    /// Pins that the iterative detector preserves the recursive one's semantics.
    #[test]
    fn test_back_edges_for_loop_classified() {
        // bb0: header  SwitchInt(true -> bb1, otherwise -> bb3)
        // bb1: body    Goto -> bb2
        // bb2: latch   Goto -> bb0   (back-edge)
        // bb3: exit     Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(3)));
        body.basic_blocks.push(goto_block(BasicBlock(2)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(return_block());

        let (headers, loop_info, invalid, multi_latch) =
            BodyEmitter::detect_loops_and_build_info(&body);

        assert_eq!(headers.len(), 1, "one loop header expected");
        assert!(headers.contains(&BasicBlock(0)));
        assert!(invalid.is_empty(), "header is a valid SwitchInt loop");
        assert!(
            multi_latch.is_empty(),
            "single-latch loop is not multi-latch"
        );

        let info = loop_info
            .get(&BasicBlock(0))
            .expect("LoopInfo for header bb0");
        assert_eq!(info.body_entry, BasicBlock(1));
        assert_eq!(info.exit, BasicBlock(3));
        assert_eq!(info.continuing, Some(BasicBlock(2)));
        assert_eq!(info.continue_target, BasicBlock(2));
    }

    /// A back-edge whose target is not a `SwitchInt` loop header (here a `Goto`
    /// self-loop) is reported as an invalid header, not a real loop.
    #[test]
    fn test_back_edges_invalid_header_rejected() {
        // bb0: Goto -> bb1
        // bb1: Goto -> bb1  (self back-edge to a non-SwitchInt block)
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks.push(goto_block(BasicBlock(1)));
        body.basic_blocks.push(goto_block(BasicBlock(1)));

        let (headers, loop_info, invalid, multi_latch) =
            BodyEmitter::detect_loops_and_build_info(&body);

        assert!(
            headers.is_empty(),
            "invalid header must be removed from headers"
        );
        assert!(
            loop_info.is_empty(),
            "no LoopInfo for a non-SwitchInt header"
        );
        assert!(
            invalid.contains(&BasicBlock(1)),
            "self back-edge to a Goto block is an invalid header"
        );
        assert!(
            multi_latch.is_empty(),
            "a single self back-edge is not multi-latch"
        );
    }

    /// A `SwitchInt` header reached by two distinct back-edges (two latches)
    /// cannot be structured for WGSL: only one latch can be the `continuing`
    /// block, so a `continue` would skip a for-loop's increment. It is reported
    /// as a multi-latch header — not a valid loop, not an invalid header.
    #[test]
    fn test_back_edges_multi_latch_reported() {
        // bb0: header  SwitchInt(true -> bb1, otherwise -> bb4)
        // bb1: inner   SwitchInt(true -> bb2, otherwise -> bb3)
        // bb2: latch A Goto -> bb0   (back-edge)
        // bb3: latch B Goto -> bb0   (back-edge)
        // bb4: exit    Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(4)));
        body.basic_blocks
            .push(loop_header_block(BasicBlock(2), BasicBlock(3)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(return_block());

        let (headers, loop_info, invalid, multi_latch) =
            BodyEmitter::detect_loops_and_build_info(&body);

        assert!(
            multi_latch.contains(&BasicBlock(0)),
            "header with two latches must be reported as multi-latch"
        );
        assert!(
            !headers.contains(&BasicBlock(0)),
            "a multi-latch header is not a compilable loop header"
        );
        assert!(
            !loop_info.contains_key(&BasicBlock(0)),
            "no LoopInfo is built for a multi-latch header"
        );
        assert!(
            !invalid.contains(&BasicBlock(0)),
            "a multi-latch header is distinct from an invalid (non-SwitchInt) header"
        );
    }

    /// A canonical single-latch while-loop — the latch jumps straight back to
    /// the header (latch == body entry has no separate increment block) — is
    /// classified with `continuing == None` and the header itself as the
    /// continue target. Pins the while-style arm of `classify_loop_header`.
    #[test]
    fn test_back_edges_while_loop_classified() {
        // bb0: header  SwitchInt(true -> bb1, otherwise -> bb2)
        // bb1: body    Goto -> bb0   (back-edge straight to header = while latch)
        // bb2: exit    Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(2)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(return_block());

        let (headers, loop_info, invalid, multi_latch) =
            BodyEmitter::detect_loops_and_build_info(&body);

        assert_eq!(headers.len(), 1, "one while-loop header expected");
        assert!(invalid.is_empty(), "while header is a valid SwitchInt loop");
        assert!(
            multi_latch.is_empty(),
            "single-latch loop is not multi-latch"
        );

        let info = loop_info
            .get(&BasicBlock(0))
            .expect("LoopInfo for while header bb0");
        assert_eq!(info.body_entry, BasicBlock(1));
        assert_eq!(info.exit, BasicBlock(2));
        assert_eq!(
            info.continuing, None,
            "a while-loop has no separate continuing block"
        );
        assert_eq!(
            info.continue_target,
            BasicBlock(0),
            "a while-loop's continue jumps back to the header"
        );
    }

    /// Constructing a `BodyEmitter` over an irreducible / condition-less
    /// back-edge (a header whose terminator is a `Goto`, not a `SwitchInt`)
    /// fails with a diagnostic that names the header and explains WGSL's
    /// structured-loop requirement, rather than a bare "invalid" rejection.
    #[test]
    fn test_irreducible_back_edge_rejected_with_structured_loop_diagnostic() {
        // bb0: Goto -> bb1
        // bb1: Goto -> bb1  (condition-less self back-edge)
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks.push(goto_block(BasicBlock(1)));
        body.basic_blocks.push(goto_block(BasicBlock(1)));

        let mut output = String::new();
        let mut source_map = Vec::new();
        let result = BodyEmitter::new(&body, &[], [256, 1, 1], &mut output, &mut source_map);

        let msg = match result {
            Ok(_) => panic!("condition-less back-edge must be rejected"),
            Err(e) => format!("{e:?}"),
        };
        assert!(
            msg.contains("bb1"),
            "diagnostic must name the offending header, got: {msg}"
        );
        assert!(
            msg.contains("structured loop"),
            "diagnostic must explain WGSL's structured-loop requirement, got: {msg}"
        );
    }

    /// Constructing a `BodyEmitter` over a multi-latch loop fails with an
    /// actionable diagnostic naming the offending header, rather than silently
    /// misclassifying the loop as while-style (which would drop the increment).
    #[test]
    fn test_multi_latch_loop_rejected_by_new() {
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(4)));
        body.basic_blocks
            .push(loop_header_block(BasicBlock(2), BasicBlock(3)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(return_block());

        let mut output = String::new();
        let mut source_map = Vec::new();
        let result = BodyEmitter::new(&body, &[], [256, 1, 1], &mut output, &mut source_map);

        let msg = match result {
            Ok(_) => panic!("multi-latch loop must be rejected"),
            Err(e) => format!("{e:?}"),
        };
        assert!(
            msg.contains("multiple latches") && msg.contains("bb0"),
            "diagnostic must name the multi-latch header, got: {msg}"
        );
    }

    /// A plain-if shape: the then-block falls through to the otherwise-block, so
    /// `then` is forward-reachable to `otherwise` (the emitter picks a plain
    /// `if`, no `else`). Pins the precomputed reachability against the old BFS.
    #[test]
    fn test_forward_reachable_then_reaches_otherwise() {
        // bb0: SwitchInt(true -> bb1, otherwise -> bb2)
        // bb1: Goto -> bb2   (then falls through to the merge)
        // bb2: Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(2)));
        body.basic_blocks.push(goto_block(BasicBlock(2)));
        body.basic_blocks.push(return_block());

        let mut out = String::new();
        let mut map = Vec::new();
        let em = BodyEmitter::new(&body, &[], [256, 1, 1], &mut out, &mut map)
            .expect("acyclic body must build");

        assert!(em.forward_reachable(BasicBlock(1), BasicBlock(2)));
        assert!(em.forward_reachable(BasicBlock(2), BasicBlock(2)));
        assert!(!em.forward_reachable(BasicBlock(2), BasicBlock(1)));
    }

    /// An if-else diamond: neither branch reaches the other, and both converge on
    /// a shared merge block — `find_merge` must return that block.
    #[test]
    fn test_find_merge_diamond() {
        // bb0: SwitchInt(true -> bb1, otherwise -> bb2)
        // bb1: Goto -> bb3 ; bb2: Goto -> bb3 ; bb3: Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(2)));
        body.basic_blocks.push(goto_block(BasicBlock(3)));
        body.basic_blocks.push(goto_block(BasicBlock(3)));
        body.basic_blocks.push(return_block());

        let mut out = String::new();
        let mut map = Vec::new();
        let em = BodyEmitter::new(&body, &[], [256, 1, 1], &mut out, &mut map)
            .expect("acyclic body must build");

        assert!(!em.forward_reachable(BasicBlock(1), BasicBlock(2)));
        assert_eq!(
            em.find_merge(BasicBlock(1), BasicBlock(2)),
            Some(BasicBlock(3))
        );
    }

    /// Diverging branches that both `Return` share no forward-reachable block, so
    /// `find_merge` returns `None` (the emitter ends the region there).
    #[test]
    fn test_find_merge_diverging_returns_none() {
        // bb0: SwitchInt(true -> bb1, otherwise -> bb2) ; bb1: Return ; bb2: Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(2)));
        body.basic_blocks.push(return_block());
        body.basic_blocks.push(return_block());

        let mut out = String::new();
        let mut map = Vec::new();
        let em = BodyEmitter::new(&body, &[], [256, 1, 1], &mut out, &mut map)
            .expect("acyclic body must build");

        assert_eq!(em.find_merge(BasicBlock(1), BasicBlock(2)), None);
    }

    /// Reachability never traverses *through* a loop header, but a header reached
    /// directly is a valid endpoint. From the latch, the header (bb0) is reachable
    /// as an endpoint, yet the post-loop exit (bb2) behind it is not.
    #[test]
    fn test_forward_reachable_stops_at_loop_header() {
        // bb0: header SwitchInt(true -> bb1, otherwise -> bb2)
        // bb1: latch  Goto -> bb0   (back-edge)
        // bb2: Return
        let mut body = Body::new(0, Span::default(), crate::mir::ExecutionModel::GpuKernel);
        body.basic_blocks
            .push(loop_header_block(BasicBlock(1), BasicBlock(2)));
        body.basic_blocks.push(goto_block(BasicBlock(0)));
        body.basic_blocks.push(return_block());

        let mut out = String::new();
        let mut map = Vec::new();
        let em = BodyEmitter::new(&body, &[], [256, 1, 1], &mut out, &mut map)
            .expect("single-latch loop must build");

        // bb0 is a direct successor of the latch → reachable endpoint.
        assert!(em.forward_reachable(BasicBlock(1), BasicBlock(0)));
        // But traversal does not continue through the header to the loop exit.
        assert!(!em.forward_reachable(BasicBlock(1), BasicBlock(2)));
    }
}
