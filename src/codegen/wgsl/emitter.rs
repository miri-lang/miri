// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR → WGSL text emitter.

use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::codegen::wgsl::types::{buffer_element, scalar, WgslScalar};
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

    pub(super) fn emit_kernel(
        &mut self,
        name: &str,
        body: &Body,
        default_workgroup_size: [u32; 3],
    ) -> Result<(), CodegenError> {
        let bindings = collect_buffer_bindings(body)?;
        self.emit_bindings(&bindings)?;
        let workgroup_size = resolve_workgroup_size(body, default_workgroup_size);
        self.emit_entry_point(name, body, &bindings, workgroup_size)
    }

    fn emit_bindings(&mut self, bindings: &[BufferBinding]) -> Result<(), CodegenError> {
        for binding in bindings {
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
                binding.element_type.name(),
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

        let mut ctx = BodyEmitter::new(body, bindings, workgroup_size, &mut self.output);
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
    read_write: bool,
}

fn collect_buffer_bindings(body: &Body) -> Result<Vec<BufferBinding>, CodegenError> {
    let mut bindings = Vec::new();
    for (next_index, param_idx) in (1..=body.arg_count).enumerate() {
        let decl = body.local_decls.get(param_idx).ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: local_decls length {} <= param_idx {}",
                body.local_decls.len(),
                param_idx
            ))
        })?;
        match decl.storage_class {
            StorageClass::GpuGlobal | StorageClass::StorageBuffer => {}
            StorageClass::UniformBuffer => {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: kernel parameter _{} uses UniformBuffer storage class, \
                     which is not yet supported (WGSL uniform buffers require fixed-size \
                     arrays; only storage buffers are emitted today)",
                    param_idx
                )));
            }
            StorageClass::Stack
            | StorageClass::GpuShared
            | StorageClass::GpuConstant
            | StorageClass::GpuPrivate => {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: kernel parameter _{} has unsupported storage class {:?}; \
                     expected GpuGlobal/StorageBuffer",
                    param_idx, decl.storage_class
                )));
            }
        }
        let read_write = body.out_params.get(param_idx - 1).copied().ok_or_else(|| {
            CodegenError::Internal(format!(
                "WGSL backend: out_params length {} < arg_count {}",
                body.out_params.len(),
                body.arg_count
            ))
        })?;
        let element_type = buffer_element(&decl.ty.kind)?;
        let var_name = decl
            .name
            .as_deref()
            .map(sanitize_identifier)
            .unwrap_or_else(|| format!("_buf{}", param_idx));
        bindings.push(BufferBinding {
            param_local: Local(param_idx),
            group: 0,
            index: next_index as u32,
            var_name,
            element_type,
            read_write,
        });
    }
    Ok(bindings)
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

struct BodyEmitter<'a> {
    body: &'a Body,
    bindings: &'a [BufferBinding],
    workgroup_size: [u32; 3],
    output: &'a mut String,
    indent: usize,
}

impl<'a> BodyEmitter<'a> {
    fn new(
        body: &'a Body,
        bindings: &'a [BufferBinding],
        workgroup_size: [u32; 3],
        output: &'a mut String,
    ) -> Self {
        Self {
            body,
            bindings,
            workgroup_size,
            output,
            indent: 1,
        }
    }

    fn write_indent(&mut self) -> Result<(), CodegenError> {
        for _ in 0..self.indent {
            self.output.push_str("    ");
        }
        Ok(())
    }

    fn emit_local_declarations(&mut self) -> Result<(), CodegenError> {
        let skip_until = self.body.arg_count + 1;
        for (i, decl) in self.body.local_decls.iter().enumerate() {
            if i == 0 || i < skip_until {
                continue;
            }
            if matches!(decl.ty.kind, TypeKind::Void) {
                continue;
            }
            let ty = scalar(&decl.ty.kind)?;
            self.write_indent()?;
            writeln!(
                self.output,
                "var {}: {} = {};",
                local_name(Local(i)),
                ty.name(),
                ty.zero_literal()
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

    fn emit_blocks(&mut self) -> Result<(), CodegenError> {
        let mut visited = std::collections::HashSet::new();
        self.emit_from(crate::mir::BasicBlock(0), None, &mut visited)
    }

    /// Emits MIR basic blocks starting at `start`, following `Goto` chains
    /// linearly and structurizing a `SwitchInt(cond, [(true, then)], otherwise=merge)`
    /// terminator into a WGSL `if` statement. Stops when reaching `stop` (if any)
    /// or a `Return`. Rejects back-edges (loops) since WGSL has no `goto` and
    /// structured loop emission is a follow-up.
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
            if !visited.insert(cur) {
                return Err(CodegenError::Internal(format!(
                    "WGSL backend: back-edge to bb{} not yet supported (loops are a follow-up)",
                    cur.0
                )));
            }
            let block = &self.body.basic_blocks[cur.0];
            for stmt in &block.statements {
                self.emit_statement(&stmt.kind)?;
            }
            let term = block.terminator.as_ref().ok_or_else(|| {
                CodegenError::Internal(format!("WGSL backend: block bb{} has no terminator", cur.0))
            })?;
            match &term.kind {
                TerminatorKind::Return => {
                    return Ok(());
                }
                TerminatorKind::Unreachable => {
                    self.write_indent()?;
                    writeln!(self.output, "// unreachable").map_err(emit_err)?;
                    return Ok(());
                }
                TerminatorKind::Goto { target } => {
                    cur = *target;
                }
                TerminatorKind::SwitchInt {
                    discr,
                    targets,
                    otherwise,
                } => {
                    if targets.len() == 1 && targets[0].0 == crate::mir::Discriminant::bool_true() {
                        let then_bb = targets[0].1;
                        let merge_bb = *otherwise;
                        let cond_str = self.render_operand(discr)?;
                        self.write_indent()?;
                        writeln!(self.output, "if (bool({})) {{", cond_str).map_err(emit_err)?;
                        self.indent += 1;
                        self.emit_from(then_bb, Some(merge_bb), visited)?;
                        self.indent -= 1;
                        self.write_indent()?;
                        writeln!(self.output, "}}").map_err(emit_err)?;
                        cur = merge_bb;
                    } else {
                        return Err(CodegenError::Internal(format!(
                            "WGSL backend: SwitchInt shape not supported (targets={:?})",
                            targets
                        )));
                    }
                }
                TerminatorKind::Call { .. }
                | TerminatorKind::GpuLaunch { .. }
                | TerminatorKind::VirtualCall { .. } => {
                    return Err(CodegenError::Internal(format!(
                        "WGSL backend: terminator {:?} not yet supported",
                        term.kind
                    )));
                }
            }
        }
    }

    fn emit_statement(&mut self, kind: &StatementKind) -> Result<(), CodegenError> {
        match kind {
            StatementKind::Assign(place, rvalue) | StatementKind::Reassign(place, rvalue) => {
                self.write_indent()?;
                let lhs = self.render_place(place)?;
                let rhs = self.render_rvalue(rvalue)?;
                writeln!(self.output, "{} = {};", lhs, rhs).map_err(emit_err)
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => Ok(()),
        }
    }

    fn render_place(&self, place: &Place) -> Result<String, CodegenError> {
        let mut rendered = match self.binding_name(place.local) {
            Some(name) => name.to_string(),
            None => local_name(place.local),
        };
        for elem in &place.projection {
            match elem {
                PlaceElem::Field(idx) => {
                    write!(rendered, ".{}", idx).map_err(emit_err)?;
                }
                PlaceElem::Index(local) => {
                    write!(rendered, "[{}]", local_name(*local)).map_err(emit_err)?;
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
            Operand::Move(place) | Operand::Copy(place) => self.render_place(place),
            Operand::Constant(c) => render_constant(c),
        }
    }

    fn render_rvalue(&self, rvalue: &Rvalue) -> Result<String, CodegenError> {
        match rvalue {
            Rvalue::Use(op) => self.render_operand(op),
            Rvalue::BinaryOp(op, lhs, rhs) => Ok(format!(
                "({} {} {})",
                self.render_operand(lhs)?,
                binop_symbol(*op)?,
                self.render_operand(rhs)?
            )),
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
            Rvalue::Len(_)
            | Rvalue::Ref(_)
            | Rvalue::Aggregate(_, _)
            | Rvalue::Phi(_)
            | Rvalue::Allocate(_, _, _) => Err(CodegenError::Internal(format!(
                "WGSL backend: rvalue {:?} not yet supported",
                rvalue
            ))),
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
/// an `i32` literal into a 64-bit storage element by mistake.
fn render_integer(i: &IntegerLiteral, ty: &TypeKind) -> String {
    let value = i.to_i128();
    match ty {
        TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U128 => format!("{}u", value),
        TypeKind::U64 => format!("{}lu", value),
        TypeKind::Int | TypeKind::I64 => format!("{}li", value),
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
