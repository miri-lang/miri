// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which device buffer each kernel binding is, and what it holds when a web
//! bundle starts.
//!
//! A web bundle runs the kernels and nothing else: no host statement executes
//! in the browser. Two rules follow.
//!
//! - A binding is identified by the persistent device buffer the host launch
//!   passes for it (its [`DeviceHandleId`]), exactly as the native runtime
//!   binds it — never by a name. Two functions may each declare `gpu var buf`,
//!   and those are two buffers; a release build keeps no local names at all.
//! - A buffer's contents must be known when the bundle is built. A declaration
//!   whose initializer needs host code to run is refused, not bundled empty.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ast::literal::Literal;
use crate::codegen::wgsl::types::{buffer_element, buffer_element_typename, WgslScalar};
use crate::diagnostics::DiagnosticCode;
use crate::error::compiler::CompilerError;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::body::DeviceHandleId;
use crate::mir::{Body, ExecutionModel, LocalDecl, Operand, StorageClass, TerminatorKind};
use crate::type_checker::GpuBufferInit;

/// Where the host launches one kernel: the device buffer passed for each of
/// the kernel's storage bindings, in binding order.
#[derive(Debug, Clone, Copy)]
pub(super) struct LaunchSite<'a> {
    pub handles: &'a [Option<DeviceHandleId>],
    pub span: Span,
}

/// What the host side of the program says about device buffers: every launch
/// site, the literal grids those launches were written with, and the local
/// that declared each device buffer.
pub(super) struct HostProgram<'a> {
    launches: HashMap<&'a str, Vec<LaunchSite<'a>>>,
    grids: HashMap<&'a str, Vec<[u32; 3]>>,
    owners: HashMap<u64, Vec<&'a LocalDecl>>,
}

impl<'a> HostProgram<'a> {
    pub(super) fn scan(mir_bodies: &'a [(String, Body)]) -> Self {
        let mut program = Self {
            launches: HashMap::new(),
            grids: HashMap::new(),
            owners: HashMap::new(),
        };
        for (_, body) in mir_bodies {
            if runs_on_host(body) {
                program.record_owners(body);
                program.record_launches(body);
                for (kernel, grid) in &body.kernel_grids {
                    program
                        .grids
                        .entry(kernel.as_str())
                        .or_default()
                        .push(*grid);
                }
            }
        }
        program
    }

    /// A local that owns a device buffer declared it; a residency-specialized
    /// parameter only borrows its caller's.
    fn record_owners(&mut self, body: &'a Body) {
        for decl in &body.local_decls {
            if let (Some(handle), false) = (decl.device_handle, decl.device_handle_borrowed) {
                self.owners.entry(handle.0).or_default().push(decl);
            }
        }
    }

    fn record_launches(&mut self, body: &'a Body) {
        let terminators = body
            .basic_blocks
            .iter()
            .filter_map(|block| block.terminator.as_ref());
        for terminator in terminators {
            let TerminatorKind::GpuLaunch {
                kernel: Operand::Constant(kernel),
                launch_args,
                ..
            } = &terminator.kind
            else {
                continue;
            };
            if let Literal::Identifier(name) = &kernel.literal {
                self.launches
                    .entry(name.as_str())
                    .or_default()
                    .push(LaunchSite {
                        handles: launch_args.arg_handles(),
                        span: terminator.span,
                    });
            }
        }
    }

    /// The one launch site of `kernel`. A bundle binds each kernel to a single
    /// set of buffers, so a kernel never launched, or launched with different
    /// buffers at two sites, cannot be bundled.
    pub(super) fn launch_of(
        &self,
        kernel: &str,
        body: &Body,
    ) -> Result<LaunchSite<'a>, CompilerError> {
        let sites = self.launches.get(kernel).map_or(&[][..], Vec::as_slice);
        let Some((first, rest)) = sites.split_first() else {
            return Err(refusal(
                format!(
                    "kernel {} is never launched, so a web bundle cannot tell which buffers it binds",
                    kernel
                ),
                body.span,
                "call the kernel from the program, or remove it",
            ));
        };
        if let Some(other) = rest.iter().find(|site| site.handles != first.handles) {
            return Err(refusal(
                format!(
                    "this launch passes kernel {} different buffers than an earlier one; \
                     a web bundle binds each kernel to one set of buffers",
                    kernel
                ),
                other.span,
                "launch the kernel on one set of gpu buffers, or give each launch its own kernel",
            ));
        }
        Ok(*first)
    }

    /// The grid every launch of `kernel` was written with, when each launch
    /// spelled the same literal grid.
    fn literal_grid(&self, kernel: &str) -> Option<[u32; 3]> {
        let grids = self.grids.get(kernel)?;
        let launch_count = self.launches.get(kernel).map_or(0, Vec::len);
        let (first, rest) = grids.split_first()?;
        (grids.len() == launch_count && rest.iter().all(|grid| grid == first)).then_some(*first)
    }

    fn owners_of(&self, handle: DeviceHandleId) -> &[&'a LocalDecl] {
        self.owners.get(&handle.0).map_or(&[][..], Vec::as_slice)
    }
}

fn runs_on_host(body: &Body) -> bool {
    match body.execution_model {
        ExecutionModel::Cpu | ExecutionModel::Async => true,
        ExecutionModel::GpuKernel | ExecutionModel::GpuDevice => false,
    }
}

/// The storage-buffer parameters of a kernel with their local indices, in
/// binding order.
pub(super) fn storage_params(body: &Body) -> impl Iterator<Item = (usize, &LocalDecl)> {
    body.local_decls
        .iter()
        .enumerate()
        .skip(1)
        .take(body.arg_count)
        .filter(|(_, decl)| {
            matches!(
                decl.storage_class,
                StorageClass::GpuGlobal | StorageClass::StorageBuffer
            )
        })
}

/// The grid a kernel dispatches — the one lowering recorded for a `forall`, or
/// the literal grid a `gpu fn` launch was written with — refusing one known
/// only at run time. The browser dispatches the grid recorded in the bundle
/// and binds no loop-bound uniform, so such a kernel would silently cover the
/// wrong range.
pub(super) fn fixed_grid(
    kernel: &str,
    body: &Body,
    grid_size: Option<[u32; 3]>,
    host: &HostProgram,
    site: &LaunchSite,
) -> Result<[u32; 3], CompilerError> {
    let has_runtime_extent = body
        .local_decls
        .iter()
        .skip(1)
        .take(body.arg_count)
        .any(|decl| decl.launch_uniform.is_some());
    if has_runtime_extent {
        return Err(refusal(
            "the bound of this parallel loop is known only at run time; a web bundle \
             dispatches a grid fixed when the bundle is built"
                .to_string(),
            site.span,
            "bound the loop with a literal or a `const`",
        ));
    }
    grid_size
        .or_else(|| host.literal_grid(kernel))
        .ok_or_else(|| {
            refusal(
                "the grid of this launch is not known when the bundle is built; a web bundle \
             dispatches a grid fixed when the bundle is built"
                    .to_string(),
                site.span,
                "express the launch as a `forall` over a literal or `const` range",
            )
        })
}

/// One device buffer of the bundle.
#[derive(Debug, Clone)]
pub(super) struct WebBuffer {
    pub name: String,
    pub element_type: &'static str,
    pub length: usize,
    pub initial_data: Vec<f64>,
    /// A sized constructor (`Array<T, N>()`): the browser zero-fills it.
    pub is_zero_filled: bool,
}

/// Every device buffer a bundle's kernels bind, under a manifest name unique
/// to its device buffer.
pub(super) struct BufferTable {
    by_name: BTreeMap<String, WebBuffer>,
    names: HashMap<u64, String>,
}

impl BufferTable {
    /// Resolves each device buffer in `bindings` (a device buffer and the
    /// kernel parameter it binds as) to its declaration and build-time
    /// contents.
    pub(super) fn build<'b>(
        bindings: impl Iterator<Item = (DeviceHandleId, &'b LocalDecl)>,
        host: &HostProgram,
        inits: &HashMap<Span, GpuBufferInit>,
        source: Option<&str>,
    ) -> Result<Self, CompilerError> {
        let mut params: BTreeMap<u64, &LocalDecl> = BTreeMap::new();
        for (handle, param) in bindings {
            params.entry(handle.0).or_insert(param);
        }
        let mut resolved = Vec::with_capacity(params.len());
        for (&handle, param) in &params {
            let (owner, init) = declared_contents(host, DeviceHandleId(handle), inits, source)?;
            let element_type = web_element_type(param, &init.name, owner)?;
            resolved.push((handle, element_type, init));
        }
        Ok(Self::name_buffers(resolved))
    }

    /// Gives each device buffer its source name, suffixing a later buffer that
    /// shares a name with an earlier one so no two collide.
    fn name_buffers(resolved: Vec<(u64, &'static str, &GpuBufferInit)>) -> Self {
        let declared: HashSet<&str> = resolved
            .iter()
            .map(|(_, _, init)| init.name.as_str())
            .collect();
        let mut table = Self {
            by_name: BTreeMap::new(),
            names: HashMap::with_capacity(resolved.len()),
        };
        for (handle, element_type, init) in resolved {
            let name = unique_name(&init.name, &declared, &table.by_name);
            table.names.insert(handle, name.clone());
            table.by_name.insert(
                name.clone(),
                WebBuffer {
                    name,
                    element_type,
                    length: init.length.unwrap_or(init.values.len()),
                    initial_data: init.values.clone(),
                    is_zero_filled: init.length.is_some(),
                },
            );
        }
        table
    }

    /// The bundle entry of a device buffer; `None` for one no kernel binds.
    pub(super) fn buffer_of(&self, handle: DeviceHandleId) -> Option<&WebBuffer> {
        self.names
            .get(&handle.0)
            .and_then(|name| self.by_name.get(name))
    }

    pub(super) fn get(&self, name: &str) -> Option<&WebBuffer> {
        self.by_name.get(name)
    }

    /// Every buffer, ordered by manifest name so identical source produces a
    /// byte-identical bundle.
    pub(super) fn iter(&self) -> impl Iterator<Item = &WebBuffer> {
        self.by_name.values()
    }
}

fn unique_name(
    base: &str,
    declared: &HashSet<&str>,
    taken: &BTreeMap<String, WebBuffer>,
) -> String {
    if !taken.contains_key(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|candidate| !taken.contains_key(candidate) && !declared.contains(candidate.as_str()))
        .unwrap_or_else(|| base.to_string())
}

/// The declaration behind a device buffer and its build-time contents. A
/// buffer moved into another binding (`gpu var b = a`) has several owners; the
/// one that declared the contents is the one that counts.
fn declared_contents<'h, 'i>(
    host: &HostProgram<'h>,
    handle: DeviceHandleId,
    inits: &'i HashMap<Span, GpuBufferInit>,
    source: Option<&str>,
) -> Result<(&'h LocalDecl, &'i GpuBufferInit), CompilerError> {
    let owners = host.owners_of(handle);
    if let Some(found) = owners
        .iter()
        .find_map(|owner| inits.get(&owner.name_span).map(|init| (*owner, init)))
    {
        return Ok(found);
    }
    let (name, span) = match owners.first() {
        Some(owner) => (declared_name(owner, source), declaration_span(owner)),
        None => ("temporary".to_string(), Span::default()),
    };
    Err(refusal(
        format!(
            "gpu buffer '{}' cannot be evaluated when the bundle is built; \
             a web bundle runs no host code, so its contents must be a literal or a sized constructor",
            name
        ),
        span,
        "initialize it with a literal of constants or `Array<T, N>()` and fill it from a kernel",
    ))
}

/// The element type the browser runtime stores the buffer as — the same
/// element the kernel declares. The runtime holds `i32`, `u32` and `f32` only.
fn web_element_type(
    param: &LocalDecl,
    name: &str,
    owner: &LocalDecl,
) -> Result<&'static str, CompilerError> {
    let internal = |err: crate::error::CodegenError| CompilerError::Codegen(err.to_string());
    let spelling = buffer_element_typename(&param.ty.kind).map_err(internal)?;
    let scalar = buffer_element(&param.ty.kind).map_err(internal)?;
    let is_browser_scalar = match scalar {
        WgslScalar::I32 | WgslScalar::U32 | WgslScalar::F32 => spelling == scalar.name(),
        WgslScalar::F16 | WgslScalar::F64 | WgslScalar::Bool => false,
    };
    if is_browser_scalar {
        return Ok(scalar.name());
    }
    Err(refusal(
        format!(
            "gpu buffer '{}' holds {} elements, which a web bundle cannot carry; \
             the browser runtime stores i32, u32 and f32 elements only",
            name, spelling
        ),
        declaration_span(owner),
        "declare the buffer with an i32, u32 or f32 element type",
    ))
}

fn declared_name(decl: &LocalDecl, source: Option<&str>) -> String {
    if let Some(name) = &decl.name {
        return name.to_string();
    }
    source
        .and_then(|src| src.get(decl.name_span.start..decl.name_span.end))
        .filter(|name| !name.is_empty())
        .unwrap_or("temporary")
        .to_string()
}

fn declaration_span(decl: &LocalDecl) -> Span {
    if decl.name_span.is_empty() {
        decl.span
    } else {
        decl.name_span
    }
}

/// A program the web target cannot bundle, reported where it is written.
fn refusal(message: String, span: Span, help: &str) -> CompilerError {
    CompilerError::Lowering(LoweringError::coded(
        DiagnosticCode::TarWebGpuUnsupported,
        message,
        span,
        Some(help.to_string()),
    ))
}
