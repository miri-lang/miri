// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR to Cranelift IR translation.
//!
//! This module translates MIR (Mid-level IR) functions into Cranelift IR,
//! which can then be compiled to machine code.

use super::translate_statement::ProjectedKind;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::closure::CaptureLayout;
use crate::codegen::cranelift::collection_header::{CollectionBase, ElementHeader};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::stable_headers::StableHeaders;
use crate::codegen::cranelift::types::translate_type;
use crate::codegen::cranelift::value_abi::body_signature;
use crate::codegen::cranelift::value_locals::{copy_chunks, copy_value, ValueLocals};
use crate::error::syntax::Span;
use crate::error::CodegenError;
use crate::mir::element_abi::element_layout;
use crate::mir::symbol::Symbol;
use crate::mir::type_facts::{TypeDefinition, TypeFacts};
use crate::mir::{BasicBlock, Body, Local, Place, PlaceElem};
use crate::runtime_fns::rt;

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{
    AbiParam, Block, Function, InstBuilder, MemFlags, Signature, SourceLoc, TrapCode, Value,
};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// Translates MIR functions to Cranelift IR.
///
/// Each `FunctionTranslator` handles a single function, managing local variables,
/// basic blocks, and the translation of statements and terminators.
///
/// The translator is constructed once per function, builds the Cranelift IR via
/// [`translate`](Self::translate), and then consumed via
/// [`into_function`](Self::into_function) to yield the built [`Function`].
pub struct FunctionTranslator<'a> {
    /// The Cranelift function being built.
    func: Function,
    /// Function builder context (reusable across functions).
    builder_ctx: FunctionBuilderContext,
    /// Default calling convention for the target.
    call_conv: CallConv,
    /// Target pointer type.
    ptr_type: cl_types::Type,
    /// Borrowed references to MIR local types to avoid cloning.
    local_types: Vec<&'a Type>,
    /// The program's settled type facts, borrowed from the backend: type
    /// definitions for layout, and the recorded generic-class instantiations a
    /// body's inline drop of a generic-class local (`let b = Box<String>()`)
    /// routes through to reach the per-instantiation `miri.Box$String.$drop`
    /// thunk that releases the concrete managed field.
    pub(crate) facts: &'a TypeFacts,
}

/// Context for module-level resources during translation.
///
/// Runtime-symbol `FuncId`s are cached in a single map keyed by symbol name.
/// `call_cached_func` declares an import the first time a symbol is touched
/// and returns the cached id on subsequent calls.
pub(crate) struct ModuleCtx<'a> {
    pub(crate) module: &'a mut ObjectModule,
    /// Whether the code being emitted carries the runtime's checking hooks.
    pub(crate) runtime_checks: RuntimeChecks,
    /// Whether each statement's instructions record the source offset its
    /// span starts at.
    pub(crate) source_locations: bool,
    pub(crate) string_literals: &'a mut BTreeMap<String, usize>,
    pub(crate) cached_funcs: HashMap<&'static str, cranelift_module::FuncId>,
    /// The per-type thunks (`miri.T.$decref` and the like) already declared in
    /// this module, by link name: a function writing into a collection of a
    /// class asks for the same thunk at every write.
    pub(crate) thunk_funcs: HashMap<String, cranelift_module::FuncId>,
    pub(crate) kernel_registry:
        &'a HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
}

/// Context for type information during translation.
pub struct TypeCtx<'a> {
    pub local_types: &'a [&'a Type],
    /// The program's settled type facts: every type definition, and the
    /// concrete type-argument tuples recorded for each generic class, which
    /// resolve a generic class's bare-generic field to the concrete type of
    /// its instantiation when deciding the field's drop semantics. Every
    /// translation site — function bodies, drop thunks, structural decref
    /// thunks and closure destructors — reads the same facts.
    pub facts: &'a TypeFacts,
    pub ptr_type: cl_types::Type,
    /// Maps each closure local to the ordered AST types of its captured variables.
    /// Used by `read_place` and `resolve_projected_type_kind` to translate
    /// `Field(i)` projections on closure locals into the correct capture type.
    pub closure_capture_ast_types: &'a HashMap<crate::mir::Local, Vec<Type>>,
    /// For scalar `out` parameters: maps each param Local to the Cranelift Variable
    /// that holds the incoming pointer. Used by the Return terminator to write back.
    pub out_param_ptr_vars: &'a HashMap<Local, Variable>,
    /// The type and value parameters the code being translated leaves open:
    /// a shared generic body's own parameters, or a generic type's in its
    /// shared drop thunk. A value held at an instantiation still naming one of
    /// these is released through the shared drop thunk; one naming any other
    /// parameter reached code that should have been monomorphized, and is
    /// reported.
    pub open_params: &'a HashSet<String>,
    /// The parameters the substitution of the body being translated bound.
    /// A value still naming one of them missed that substitution, and is
    /// reported rather than released through a shared drop function.
    pub bound_params: &'a HashSet<String>,
    /// The locals of the code being translated that hold a bitwise value as
    /// its bytes, and how it holds any other type.
    pub values: &'a ValueLocals<'a>,
    /// The collection headers the code being translated reads once per loop
    /// rather than at every element access.
    pub stable_headers: &'a StableHeaders,
}

impl TypeCtx<'_> {
    /// What the optionals of the code being translated are laid out against:
    /// the program's type table and the parameters this code leaves open.
    pub fn layout_scope(&self) -> crate::mir::option_layout::LayoutScope<'_> {
        crate::mir::option_layout::LayoutScope::new(self.facts.definitions(), self.open_params)
    }

    /// The layout a value of `kind` is held in as its bytes by the code being
    /// translated, or `None` when it is held as a word.
    ///
    /// A body answers for itself; code with no body of its own — a thunk, a
    /// destructor — holds values the way every program does.
    pub fn value_layout(&self, kind: &TypeKind) -> Option<crate::mir::type_facts::ValueLayout> {
        match self.values.body() {
            Some(body) => body.value_layout(kind),
            None => crate::mir::type_facts::held_value_layout(kind, self.layout_scope()),
        }
    }
}

/// One Cranelift runtime call site: which symbol to declare-and-call, its
/// parameter / return types, and the argument values to pass.
pub(crate) struct CallSite<'a> {
    pub(crate) name: &'static str,
    pub(crate) param_types: &'a [cranelift_codegen::ir::Type],
    pub(crate) return_types: &'a [cranelift_codegen::ir::Type],
    pub(crate) args: &'a [Value],
}

/// What compiled code does about the runtime's checks: the heap guard, the
/// leak check, and the allocation and reference-count counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeChecks {
    /// Every hook is emitted behind a test of the runtime's tracking state, so
    /// one binary runs checked or plain as its environment asks.
    Observable,
    /// No hook is emitted and the tracking state is never read: a release
    /// build. A call into the runtime left on a path through a loop makes the
    /// register allocator treat every value live across it as clobbered, so a
    /// hook skipped at run time still costs the loop its registers. The entry
    /// point instead tells the runtime once, at startup, that the checks are
    /// absent, and the runtime keeps its own hooks off to match.
    CompiledOut,
}

/// How a user function's code is emitted beyond its MIR: whether it keeps the
/// runtime's checking hooks, whether it is the program's entry point, which
/// is where a build that leaves them out says so to the runtime and where a
/// profile build hands the runtime its function table, and whether its
/// instructions record the source offset they were generated from.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Emission {
    pub(crate) runtime_checks: RuntimeChecks,
    pub(crate) is_entry: bool,
    pub(crate) source_locations: bool,
    pub(crate) profile_table: Option<ProfileTable>,
}

/// The table of compiled functions a profile build defines for the runtime's
/// sampler: its data object, and how many functions it lists.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProfileTable {
    pub(crate) data: cranelift_module::DataId,
    pub(crate) count: usize,
}

/// Build an empty `ModuleCtx` with an unpopulated FuncId cache.
/// Caches populate lazily as code generation needs each runtime symbol.
pub(crate) fn empty_module_ctx<'a>(
    module: &'a mut ObjectModule,
    string_literals: &'a mut BTreeMap<String, usize>,
    kernel_registry: &'a HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
) -> ModuleCtx<'a> {
    ModuleCtx {
        module,
        runtime_checks: RuntimeChecks::Observable,
        source_locations: false,
        string_literals,
        cached_funcs: HashMap::new(),
        thunk_funcs: HashMap::new(),
        kernel_registry,
    }
}

/// What binding a body's entry-block parameters to its locals reads.
#[derive(Clone, Copy)]
struct EntryBinding<'e> {
    body: &'e Body,
    locals: &'e HashMap<Local, Variable>,
    values: &'e ValueLocals<'e>,
    ptr_type: cl_types::Type,
}

/// Normalized element type for runtime decref/clone helper dispatch.
///
/// Collapses canonical `TypeKind::{List,Array,Set,Map}` and the post-
/// normalization `TypeKind::Custom(name, _)` form (where
/// `BuiltinCollectionKind::from_name(name)` is `Some`) into a single
/// `Builtin(BuiltinCollectionKind)` variant so dispatch sites match once
/// per shape instead of duplicating arms for each spelling.
#[derive(Debug, Clone, Copy)]
pub enum ElementShape<'a> {
    /// String element — uses `miri_rt_string_decref_element`.
    String,
    /// Built-in collection element (List/Array/Set/Map) — uses the matching
    /// `miri_rt_{kind}_decref_element` helper.
    Builtin(BuiltinCollectionKind),
    /// User-defined class — uses the `miri.TypeName.$decref` thunk; clone via
    /// `miri.TypeName.$clone` only when the class implements `Cloneable`.
    UserClass(&'a str),
    /// Anything else: primitives (no decref) and structural values (Option,
    /// tuple, struct, Result) whose decref comes from the element's kind
    /// rather than its shape.
    Other,
}

impl<'a> FunctionTranslator<'a> {
    /// Create a new function translator.
    ///
    /// # Arguments
    ///
    /// * `isa` - The target instruction set architecture
    /// * `body` - The MIR body whose local types will be cached
    /// * `facts` - The program's settled type facts, borrowed from the backend
    pub fn new(isa: &Arc<dyn TargetIsa>, body: &'a Body, facts: &'a TypeFacts) -> Self {
        let func = Function::new();
        let builder_ctx = FunctionBuilderContext::new();
        let ptr_type = isa.pointer_type();

        // Borrow local types for fast lookup during translation (avoids cloning)
        let local_types = body.local_decls.iter().map(|d| &d.ty).collect();

        Self {
            func,
            builder_ctx,
            call_conv: isa.default_call_conv(),
            ptr_type,
            local_types,
            facts,
        }
    }
    /// Translate a MIR function body to Cranelift IR, emitted as `emission`
    /// says.
    pub(crate) fn translate(
        &mut self,
        body: &Body,
        module: &mut ObjectModule,
        string_literals: &mut BTreeMap<String, usize>,
        kernel_registry: &HashMap<String, crate::codegen::cranelift::gpu_launch::KernelEmit>,
        emission: Emission,
    ) -> Result<(), CodegenError> {
        self.build_signature(body)?;
        let values = ValueLocals::of(body)?;
        let mut builder = FunctionBuilder::new(&mut self.func, &mut self.builder_ctx);

        let (locals, blocks) = Self::declare_locals_and_blocks(&mut builder, body, self.ptr_type);
        let entry = EntryBinding {
            body,
            locals: &locals,
            values: &values,
            ptr_type: self.ptr_type,
        };
        let out_param_ptr_vars = Self::setup_entry_params(&mut builder, &entry, &blocks);

        let mut module_ctx = empty_module_ctx(module, string_literals, kernel_registry);
        module_ctx.runtime_checks = emission.runtime_checks;
        module_ctx.source_locations = emission.source_locations;
        if emission.is_entry && emission.runtime_checks == RuntimeChecks::CompiledOut {
            Self::call_rt_checks_compiled_out(&mut builder, &mut module_ctx)?;
        }
        if let Some(table) = emission.profile_table.filter(|_| emission.is_entry) {
            Self::call_rt_profile_init(&mut builder, &mut module_ctx, table, self.ptr_type)?;
        }
        let stable_headers = StableHeaders::declare(&mut builder, body, self.ptr_type)?;
        let type_ctx = TypeCtx {
            local_types: &self.local_types,
            facts: self.facts,
            ptr_type: self.ptr_type,
            closure_capture_ast_types: &body.closure_capture_types,
            out_param_ptr_vars: &out_param_ptr_vars,
            open_params: &body.open_params,
            bound_params: &body.bound_params,
            values: &values,
            stable_headers: &stable_headers,
        };

        Self::translate_blocks(
            &mut builder,
            &mut module_ctx,
            body,
            &locals,
            &blocks,
            &out_param_ptr_vars,
            &type_ctx,
            self.ptr_type,
        )?;

        builder.seal_all_blocks();
        builder.finalize();
        Ok(())
    }

    /// Declare a Cranelift variable per MIR local and a Cranelift block per
    /// MIR basic block. The entry block (idx 0) is wired up with function
    /// parameters.
    fn declare_locals_and_blocks(
        builder: &mut FunctionBuilder,
        body: &Body,
        ptr_type: cl_types::Type,
    ) -> (HashMap<Local, Variable>, HashMap<BasicBlock, Block>) {
        let mut locals: HashMap<Local, Variable> = HashMap::with_capacity(body.local_decls.len());
        let mut blocks: HashMap<BasicBlock, Block> =
            HashMap::with_capacity(body.basic_blocks.len());
        for (idx, local_decl) in body.local_decls.iter().enumerate() {
            let cl_type = translate_type(&local_decl.ty, ptr_type);
            let var = builder.declare_var(cl_type);
            locals.insert(Local(idx), var);
        }
        for idx in 0..body.basic_blocks.len() {
            let cl_block = builder.create_block();
            blocks.insert(BasicBlock(idx), cl_block);
            if idx == 0 {
                builder.append_block_params_for_function_params(cl_block);
            }
        }
        (locals, blocks)
    }

    /// Bind entry-block parameters to their locals. Scalar `out` params arrive
    /// as pointers — the pointer is saved into a `Variable` for later writeback
    /// at Return; the value load is deferred to the main loop so the entry
    /// block stays Pristine until `switch_to_block` runs.
    ///
    /// A result held as its bytes arrives first, as the address of the
    /// caller's slot, and becomes `_0`'s `Variable`. A value parameter is the
    /// address of its bytes, and so is a value `out` parameter: it is written
    /// in place, with nothing to write back.
    fn setup_entry_params(
        builder: &mut FunctionBuilder,
        entry: &EntryBinding,
        blocks: &HashMap<BasicBlock, Block>,
    ) -> HashMap<Local, Variable> {
        let EntryBinding {
            body,
            locals,
            values,
            ptr_type,
        } = *entry;
        let mut out_param_ptr_vars: HashMap<Local, Variable> = HashMap::new();
        let Some(&entry_block) = blocks.get(&BasicBlock(0)) else {
            return out_param_ptr_vars;
        };
        builder.switch_to_block(entry_block);
        let mut params_vec: Vec<Value> = builder.block_params(entry_block).to_vec();
        if values.layout(Local(0)).is_some() && !params_vec.is_empty() {
            let return_slot = params_vec.remove(0);
            if let Some(&var) = locals.get(&Local(0)) {
                builder.def_var(var, return_slot);
            }
        }
        let written_back = crate::mir::body::written_back_out_params(body);
        for (i, param_val) in params_vec.into_iter().enumerate() {
            let local = Local(i + 1);
            if written_back.contains(&local) && values.layout(local).is_none() {
                let ptr_var = builder.declare_var(ptr_type);
                builder.def_var(ptr_var, param_val);
                out_param_ptr_vars.insert(local, ptr_var);
            } else if let Some(&var) = locals.get(&local) {
                builder.def_var(var, param_val);
            }
        }
        out_param_ptr_vars
    }

    /// Lower MIR statements + terminator for every basic block. Block 0 also
    /// runs the deferred scalar-out-param load and closure-capture extraction
    /// (must happen after `switch_to_block` per Cranelift's block-state rules).
    #[allow(clippy::too_many_arguments)]
    fn translate_blocks(
        builder: &mut FunctionBuilder,
        module_ctx: &mut ModuleCtx,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        blocks: &HashMap<BasicBlock, Block>,
        out_param_ptr_vars: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        for (idx, block_data) in body.basic_blocks.iter().enumerate() {
            let block = blocks[&BasicBlock(idx)];
            builder.switch_to_block(block);

            if idx == 0 {
                Self::define_value_slots(builder, body, locals, type_ctx.values, ptr_type);
                Self::load_scalar_out_params(builder, body, locals, out_param_ptr_vars, ptr_type);
                if !body.env_capture_locals.is_empty() {
                    Self::load_closure_captures(builder, body, locals, ptr_type);
                }
            }

            type_ctx.stable_headers.enter_block(idx);
            for (position, stmt) in block_data.statements.iter().enumerate() {
                type_ctx.stable_headers.enter_statement(position);
                Self::mark_source(builder, module_ctx, stmt.span);
                Self::translate_statement(builder, module_ctx, stmt, body, locals, type_ctx)?;
            }
            type_ctx
                .stable_headers
                .enter_statement(block_data.statements.len());
            Self::read_stable_headers(builder, idx, locals, type_ctx)?;
            if let Some(ref terminator) = block_data.terminator {
                Self::mark_source(builder, module_ctx, terminator.span);
                Self::translate_terminator(
                    builder, module_ctx, terminator, body, locals, blocks, type_ctx,
                )?;
            }
        }
        Ok(())
    }

    /// Record `span`'s start as the source of the instructions built next,
    /// when the module asks for source locations. A span covering no text
    /// was synthesized rather than written, so the instructions keep the
    /// location of the statement before them.
    fn mark_source(builder: &mut FunctionBuilder, module_ctx: &ModuleCtx, span: Span) {
        if !module_ctx.source_locations || span.is_empty() {
            return;
        }
        let Ok(offset) = u32::try_from(span.start) else {
            return;
        };
        let location = SourceLoc::new(offset);
        if !location.is_default() {
            builder.set_srcloc(location);
        }
    }

    /// Load initial scalar `out` param values from their caller-provided
    /// pointers into the matching `Local` Variables.
    fn load_scalar_out_params(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        out_param_ptr_vars: &HashMap<Local, Variable>,
        ptr_type: cl_types::Type,
    ) {
        for (&out_local, &ptr_var) in out_param_ptr_vars {
            let ptr = builder.use_var(ptr_var);
            let val_cl_type = translate_type(&body.local_decls[out_local.0].ty, ptr_type);
            let val = builder.ins().load(val_cl_type, MemFlags::new(), ptr, 0);
            if let Some(&var) = locals.get(&out_local) {
                builder.def_var(var, val);
            }
        }
    }

    /// Closure bodies: load captured values from `env_ptr` (Local 1) at the
    /// top of the entry block, each at its own type from where
    /// [`CaptureLayout`] places it.
    fn load_closure_captures(
        builder: &mut FunctionBuilder,
        body: &Body,
        locals: &HashMap<Local, Variable>,
        ptr_type: cl_types::Type,
    ) {
        let env_ptr_var = locals[&Local(1)];
        let env_ptr_val = builder.use_var(env_ptr_var);
        let layout = CaptureLayout::of_body(body, ptr_type);
        for (&cap_local, &offset) in body.env_capture_locals.iter().zip(&layout.offsets) {
            let Some(&cap_var) = locals.get(&cap_local) else {
                continue;
            };
            let cap_ty = &body.local_decls[cap_local.0].ty;
            // A value held as its bytes is copied out of the environment
            // into the capture's own slot.
            if let Some(value) = body.value_layout(&cap_ty.kind) {
                let source = builder.ins().iadd_imm(env_ptr_val, i64::from(offset));
                let slot = builder.use_var(cap_var);
                copy_value(builder, source, slot, &value);
                continue;
            }
            let cap_cl_type = translate_type(cap_ty, ptr_type);
            let cap_val = builder
                .ins()
                .load(cap_cl_type, MemFlags::new(), env_ptr_val, offset);
            builder.def_var(cap_var, cap_val);
        }
    }
    /// Get the function signature.
    pub fn signature(&self) -> &Signature {
        &self.func.signature
    }
    /// Consume the translator and return the built function.
    pub fn into_function(self) -> Function {
        self.func
    }
    /// Build the function signature from the MIR body.
    pub(crate) fn build_signature(&mut self, body: &Body) -> Result<(), CodegenError> {
        self.func.signature = body_signature(body, self.call_conv, self.ptr_type);
        Ok(())
    }
    /// Read a value from a place.
    ///
    /// Each projection is resolved against the type reached so far, not against
    /// the root local's type: `words[0].text` takes its field offset from the
    /// element's own layout, where the collection's layout would name a
    /// different slot and hand back a neighbouring field.
    ///
    /// When `expected_ty` is Some and the place has a Field projection that would
    /// normally load as `ptr_type` (e.g., enum or Option payload), use the expected type's
    /// Cranelift width instead. This prevents field-type erasure for payloads.
    pub(crate) fn read_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
        expected_ty: Option<&Type>,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        if let Some(value) = Self::read_value_local_place(builder, ctx, place, locals, type_ctx)? {
            return Ok(value);
        }
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;

        let mut value = builder.use_var(*var);
        // The type at the current projection depth, advanced in step with
        // `value` so every field offset is read out of the right layout.
        let mut current_type: Type = type_ctx.local_types[place.local.0].clone();
        // When the previous projection was an `Index` onto an inline vector
        // element, `value` holds the element *address* and this carries that
        // element's vector type so a following `Field` loads the scalar from it.
        let mut inline_elem: Option<TypeKind> = None;

        let mut walk = ProjectedKind::new(place, type_ctx);
        let last = place.projection.len().saturating_sub(1);
        for (position, proj) in place.projection.iter().enumerate() {
            let reached = walk.step(proj, type_ctx)?;
            let read_expected = expected_ty.filter(|_| position == last);
            match proj {
                PlaceElem::Deref => {
                    value = builder.ins().load(ptr_type, MemFlags::new(), value, 0);
                    inline_elem = None;
                }
                PlaceElem::Field(idx) => {
                    let field = FieldRead {
                        owner: &current_type,
                        idx: *idx,
                        reached,
                        expected: read_expected,
                    };
                    value = match inline_elem.take() {
                        Some(vec_kind) => Self::load_inline_element_field(
                            builder, value, &vec_kind, *idx, type_ctx,
                        )?,
                        // The payload of an optional held as its payload's
                        // pointer is that same word: there is no box to open.
                        None if Self::opens_nullable_pointer(
                            &current_type.kind,
                            *idx,
                            type_ctx,
                        )? =>
                        {
                            value
                        }
                        None => {
                            Self::read_owned_field(builder, value, field, expected_ty, type_ctx)?
                        }
                    };
                    current_type = Self::type_after_projection(reached, &current_type);
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals.get(local).ok_or_else(|| {
                        CodegenError::Internal(format!("Unknown index local: {:?}", local))
                    })?;
                    let idx_val = builder.use_var(*idx_var);
                    let elem_type =
                        Self::resolve_collection_elem_type_as_type(&current_type).cloned();
                    let base = Self::collection_base(builder, place, position, value, type_ctx);
                    value = Self::translate_collection_index_read(
                        builder,
                        ctx,
                        base,
                        idx_val,
                        &current_type,
                        type_ctx,
                    )?;
                    inline_elem = Self::inline_vector_kind(elem_type.as_ref(), type_ctx);
                    if let Some(elem_type) = elem_type {
                        current_type = elem_type;
                    }
                }
            }
        }

        Ok(value)
    }

    /// The kind of `element` when it is an inline vector, which an index read
    /// yields as the address of its components.
    fn inline_vector_kind(element: Option<&Type>, type_ctx: &TypeCtx) -> Option<TypeKind> {
        element
            .map(|t| t.kind.clone())
            .filter(|k| inline_vec_element_layout(k, type_ctx).is_some())
    }

    /// Read `place` when it is rooted in a value local: a scalar inside it is
    /// loaded, a whole value is its address. `None` for any other place.
    fn read_value_local_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        let is_indexed = place
            .projection
            .iter()
            .any(|elem| matches!(elem, PlaceElem::Index(_)));
        if is_indexed && type_ctx.values.layout(place.local).is_some() {
            return Self::read_indexed_value_place(builder, ctx, place, locals, type_ctx).map(Some);
        }
        Ok(Self::value_place_address(builder, place, locals, type_ctx)?
            .map(|(address, spot)| Self::read_value_spot(builder, address, spot)))
    }

    /// Whether a projection reaching a value of `reached` kind reaches a value
    /// held as its bytes, which a read yields as its address.
    ///
    /// An enum's payload names no kind of its own; the destination it is read
    /// into, `expected`, says what it holds.
    // TODO: a payload projection is a bare `Field(k)` with no variant, so
    // neither MIR nor this backend can name the payload's type and the
    // destination stands in for it. A place element that carries the variant
    // would let `PlaceTypes` answer, and this guess would go.
    pub(crate) fn reaches_held_value(
        reached: &TypeKind,
        expected: Option<&Type>,
        type_ctx: &TypeCtx,
    ) -> bool {
        match reached {
            TypeKind::Error => {
                expected.is_some_and(|expected| type_ctx.value_layout(&expected.kind).is_some())
            }
            reached => type_ctx.value_layout(reached).is_some(),
        }
    }

    /// Read field `field.idx` out of the aggregate at `value`: a value held
    /// inline in it as the address of its bytes, anything else loaded.
    fn read_owned_field(
        builder: &mut FunctionBuilder,
        value: Value,
        field: FieldRead,
        expected_ty: Option<&Type>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        if !Self::reaches_held_value(field.reached, field.expected, type_ctx) {
            return Self::load_owned_field(
                builder,
                value,
                field.owner,
                field.idx,
                expected_ty,
                type_ctx,
            );
        }
        let (offset, _) = layout::field_layout(
            &field.owner.kind,
            field.idx,
            type_ctx.layout_scope(),
            type_ctx.ptr_type,
        )?;
        Ok(builder.ins().iadd_imm(value, i64::from(offset)))
    }

    /// Load field `idx` out of an inline vector element whose address is `value`.
    ///
    /// A collection lays vector elements out inline, so an index read yields the
    /// element's address rather than a pointer stored in the slot; the field
    /// scalar sits at its offset inside that element.
    fn load_inline_element_field(
        builder: &mut FunctionBuilder,
        value: Value,
        vec_kind: &TypeKind,
        idx: usize,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let (offset, member) =
            layout::field_layout(vec_kind, idx, type_ctx.layout_scope(), type_ctx.ptr_type)?;
        let field_ty = member_word(&member, vec_kind, idx)?;
        Ok(builder.ins().load(field_ty, MemFlags::new(), value, offset))
    }

    /// Load field `idx` out of the aggregate of type `owner_type` at `value`.
    ///
    /// `owner_type` is the type reached at this projection depth, so a field
    /// read off a collection element takes its offset from the element rather
    /// than from the collection.
    fn load_owned_field(
        builder: &mut FunctionBuilder,
        value: Value,
        owner_type: &Type,
        idx: usize,
        expected_ty: Option<&Type>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        if matches!(owner_type.kind, TypeKind::Function(_)) {
            // Closure env field: capture `idx` lives at
            // payload_ptr + (idx+2)*ptr_size (slot 0=fn_ptr, slot 1=dtor_ptr).
            let offset = (idx as i64 + 2) * ptr_type.bytes() as i64;
            return Ok(builder
                .ins()
                .load(ptr_type, MemFlags::new(), value, offset as i32));
        }
        let (offset, member) =
            layout::field_layout(&owner_type.kind, idx, type_ctx.layout_scope(), ptr_type)?;
        let mut field_ty = match member {
            layout::Member::Word(ty) => ty,
            // A value held inline in its holder is read as its address.
            layout::Member::Inline(_) => {
                return Ok(builder.ins().iadd_imm(value, i64::from(offset)));
            }
        };
        if let Some(expected) = expected_ty {
            if Self::field_width_comes_from_expected_ty(&owner_type.kind, type_ctx) {
                field_ty =
                    crate::codegen::cranelift::types::translate_type_kind(&expected.kind, ptr_type);
            }
        }
        Ok(builder.ins().load(field_ty, MemFlags::new(), value, offset))
    }

    /// True when a field of `owner_kind` must be loaded at the destination's
    /// width instead of the one `field_layout` reports.
    ///
    /// An enum or Option payload slot is at least pointer-sized whatever it
    /// holds, and `field_layout` reports the slot's word rather than the
    /// field's type; a generic parameter carries no width of its own. Loading
    /// at the reported width erases the field's real type. A generic enum such
    /// as `Result<T, E>` is the case that makes this load-bearing: the binding
    /// is resolved where the aggregate definition is still generic.
    fn field_width_comes_from_expected_ty(owner_kind: &TypeKind, type_ctx: &TypeCtx) -> bool {
        match owner_kind {
            TypeKind::Option(_) => true,
            TypeKind::Custom(name, _) => type_ctx
                .facts
                .definitions()
                .get(name)
                .map(|def| matches!(def, TypeDefinition::Enum(_)))
                .unwrap_or(false),
            TypeKind::Generic(_, _, _) => true,
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
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::OneOf(_)
            | TypeKind::Set(_)
            | TypeKind::Result(_, _)
            | TypeKind::Future(_)
            | TypeKind::Function(_)
            | TypeKind::Meta(_)
            | TypeKind::Void
            | TypeKind::Error
            | TypeKind::Linear(_) => false,
        }
    }
    /// Cast a value between Cranelift types.
    ///
    /// When `is_unsigned` is true, integer widening uses zero-extension
    /// and float-to-int uses unsigned saturation. Defaults to signed.
    pub(crate) fn cast_value_with_sign(
        builder: &mut FunctionBuilder,
        value: Value,
        from_ty: cranelift_codegen::ir::Type,
        to_ty: cranelift_codegen::ir::Type,
        is_unsigned: bool,
    ) -> Result<Value, CodegenError> {
        if from_ty == to_ty {
            return Ok(value);
        }
        if Self::is_int128_float_pair(from_ty, to_ty) {
            // The backend has no instruction for this conversion; only an
            // explicit cast, which can reach the runtime, lowers it.
            return Err(CodegenError::Internal(format!(
                "Unsupported implicit cast from {} to {}: a 128-bit integer converts \
                 to or from a float only through an explicit cast",
                from_ty, to_ty
            )));
        }

        if from_ty.is_float() && to_ty.is_float() {
            if from_ty.bytes() > to_ty.bytes() {
                Ok(builder.ins().fdemote(to_ty, value))
            } else {
                Ok(builder.ins().fpromote(to_ty, value))
            }
        } else if from_ty.is_int() && to_ty.is_int() {
            if from_ty.bytes() > to_ty.bytes() {
                Ok(builder.ins().ireduce(to_ty, value))
            } else if is_unsigned {
                Ok(builder.ins().uextend(to_ty, value))
            } else {
                Ok(builder.ins().sextend(to_ty, value))
            }
        } else if from_ty.is_float() && to_ty.is_int() {
            // Saturating float-to-int avoids trapping on NaN/overflow
            if is_unsigned {
                Ok(builder.ins().fcvt_to_uint_sat(to_ty, value))
            } else {
                Ok(builder.ins().fcvt_to_sint_sat(to_ty, value))
            }
        } else if from_ty.is_int() && to_ty.is_float() {
            if is_unsigned {
                Ok(builder.ins().fcvt_from_uint(to_ty, value))
            } else {
                Ok(builder.ins().fcvt_from_sint(to_ty, value))
            }
        } else {
            Err(CodegenError::Internal(format!(
                "Unsupported implicit cast from {} to {}",
                from_ty, to_ty
            )))
        }
    }
    /// Assign a value to a place.
    pub(crate) fn assign_to_place(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        value: Value,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if place.projection.is_empty() {
            let var = locals.get(&place.local).ok_or_else(|| {
                CodegenError::Internal(format!("Unknown local: {:?}", place.local))
            })?;
            builder.def_var(*var, value);
            return Ok(());
        }
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let base_addr = builder.use_var(*var);
        let (addr, current_type) =
            Self::walk_projection_path(builder, ctx, place, base_addr, locals, type_ctx)?;
        Self::store_at_last_projection(
            builder,
            ctx,
            place,
            value,
            addr,
            &current_type,
            locals,
            type_ctx,
        )
    }

    /// The address of the value held inline at the projected `place`, outside
    /// every value local: a value in a class or struct field, a collection
    /// element, a payload.
    pub(crate) fn held_place_address(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let var = locals
            .get(&place.local)
            .ok_or_else(|| CodegenError::Internal(format!("Unknown local: {:?}", place.local)))?;
        let base_addr = builder.use_var(*var);
        let (addr, holder) =
            Self::walk_projection_path(builder, ctx, place, base_addr, locals, type_ctx)?;
        match place.projection.last() {
            Some(PlaceElem::Field(idx)) => {
                let (offset, _) = layout::field_layout(
                    &holder.kind,
                    *idx,
                    type_ctx.layout_scope(),
                    type_ctx.ptr_type,
                )?;
                Ok(builder.ins().iadd_imm(addr, i64::from(offset)))
            }
            Some(PlaceElem::Index(index)) => {
                let index_var = locals.get(index).ok_or_else(|| {
                    CodegenError::Internal(format!("Unknown index local: {index:?}"))
                })?;
                let index_val = builder.use_var(*index_var);
                let base = Self::last_collection_base(builder, place, addr, type_ctx);
                Self::translate_collection_index_read(
                    builder, ctx, base, index_val, &holder, type_ctx,
                )
            }
            Some(PlaceElem::Deref) | None => Err(CodegenError::Internal(format!(
                "`{place}` reaches no value held inline in a holder"
            ))),
        }
    }

    /// Walk every projection on `place` *except the last*, advancing `addr`
    /// and tracking the type at the current depth. Returns the address that
    /// the final projection should consume and the type of the value at that
    /// address.
    fn walk_projection_path(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        base_addr: Value,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(Value, Type), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let mut addr = base_addr;
        let mut current_type: Type = type_ctx.local_types[place.local.0].clone();

        let mut walk = ProjectedKind::new(place, type_ctx);
        let prefix = &place.projection[..place.projection.len() - 1];
        for (position, proj) in prefix.iter().enumerate() {
            let reached = walk.step(proj, type_ctx)?;
            match proj {
                PlaceElem::Deref => {
                    addr = builder.ins().load(ptr_type, MemFlags::new(), addr, 0);
                }
                PlaceElem::Field(idx)
                    if Self::opens_nullable_pointer(&current_type.kind, *idx, type_ctx)? =>
                {
                    // The word reached so far is already the payload pointer.
                    current_type = Self::type_after_projection(reached, &current_type);
                }
                PlaceElem::Field(idx) => {
                    let (offset, _) = layout::field_layout(
                        &current_type.kind,
                        *idx,
                        type_ctx.layout_scope(),
                        ptr_type,
                    )?;
                    addr = builder.ins().iadd_imm(addr, offset as i64);
                    current_type = Self::type_after_projection(reached, &current_type);
                    // A field that owns its own allocation stores a *pointer* to
                    // it, so reaching what lives inside that field means loading
                    // the pointer rather than adding another offset to the
                    // object holding it. Skipping the load writes the value into
                    // the outer object's own slot: the store lands one level too
                    // high, and what it overwrote is leaked. A field is a
                    // one-word slot, so a vector held in one — a class or
                    // struct field, a tuple element, a boxed optional's or an
                    // enum's payload — is such a pointer too.
                    if crate::mir::rc::is_slot_counted(&current_type.kind, type_ctx.layout_scope())
                    {
                        addr = builder.ins().load(ptr_type, MemFlags::new(), addr, 0);
                    }
                }
                PlaceElem::Index(local) => {
                    let idx_var = locals.get(local).ok_or_else(|| {
                        CodegenError::Internal(format!("Unknown index local: {:?}", local))
                    })?;
                    let idx_val = builder.use_var(*idx_var);
                    let base = Self::collection_base(builder, place, position, addr, type_ctx);
                    addr = Self::translate_collection_index_read(
                        builder,
                        ctx,
                        base,
                        idx_val,
                        &current_type,
                        type_ctx,
                    )?;
                    if let Some(elem_type) =
                        Self::resolve_collection_elem_type_as_type(&current_type)
                    {
                        current_type = elem_type.clone();
                    }
                }
            }
        }
        Ok((addr, current_type))
    }

    /// Whether projecting field `idx` out of a value of `owner` reads the
    /// payload of an optional held as its payload's own pointer — a projection
    /// that moves no address, since the optional's word is the payload.
    ///
    /// Such an optional has exactly one field; any other index means MIR and
    /// the layout disagree about the value, and is reported.
    fn opens_nullable_pointer(
        owner: &TypeKind,
        idx: usize,
        type_ctx: &TypeCtx,
    ) -> Result<bool, CodegenError> {
        if !Self::nullable_pointer_layout(owner, type_ctx)? {
            return Ok(false);
        }
        if idx != 0 {
            return Err(CodegenError::Internal(format!(
                "field {idx} is projected from `{owner:?}`, an optional whose only field is \
                 its payload"
            )));
        }
        Ok(true)
    }

    /// The type a projection walk reached, as `kind`, or `fallback` when the
    /// walk could not resolve it.
    ///
    /// Walking the prefix through the one resolver that knows how a field type
    /// is read out of a struct, a class, or a generic instantiation keeps the
    /// store path from growing a second, divergent copy of those rules.
    fn type_after_projection(kind: &TypeKind, fallback: &Type) -> Type {
        if matches!(kind, TypeKind::Error) {
            return fallback.clone();
        }
        Type::new(kind.clone(), fallback.span)
    }

    /// Apply the final projection on `place` as a store of `value`.
    /// `addr` and `current_type` come from `walk_projection_path`.
    #[allow(clippy::too_many_arguments)]
    fn store_at_last_projection(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        place: &Place,
        value: Value,
        addr: Value,
        current_type: &Type,
        locals: &HashMap<Local, Variable>,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let last_proj = place.projection.last().ok_or_else(|| {
            CodegenError::Internal(
                "assign_to_place: empty projection after non-empty check".to_string(),
            )
        })?;
        match last_proj {
            PlaceElem::Deref => {
                builder.ins().store(MemFlags::new(), value, addr, 0);
            }
            PlaceElem::Field(idx) => {
                if Self::opens_nullable_pointer(&current_type.kind, *idx, type_ctx)? {
                    return Err(CodegenError::Internal(format!(
                        "a store reaches into the payload of `{:?}`, an optional held as its \
                         payload's pointer, which has no slot of its own to write",
                        current_type.kind
                    )));
                }
                let (offset, member) = layout::field_layout(
                    &current_type.kind,
                    *idx,
                    type_ctx.layout_scope(),
                    type_ctx.ptr_type,
                )?;
                let field_ty = match &member {
                    layout::Member::Word(ty) => *ty,
                    // A value held inline: `value` is the address of the
                    // bytes to copy in.
                    layout::Member::Inline(value_layout) => {
                        let dest = builder.ins().iadd_imm(addr, i64::from(offset));
                        copy_value(builder, value, dest, value_layout);
                        return Ok(());
                    }
                };
                // Storing into an inline vector component: narrow/extend the
                // value to the component width. `arr[i].x` is typed without a
                // concrete scalar width upstream, so an `f64` literal can reach
                // an `f32` field; an unnarrowed store would write past it.
                let is_vector_field = crate::ast::types::vec_type_dim(&current_type.kind).is_some();
                let mut value = value;
                if is_vector_field {
                    let from_ty = builder.func.dfg.value_type(value);
                    if from_ty != field_ty {
                        value =
                            Self::cast_value_with_sign(builder, value, from_ty, field_ty, false)?;
                    }
                }
                builder.ins().store(MemFlags::new(), value, addr, offset);
            }
            PlaceElem::Index(local) => {
                let idx_var = locals.get(local).ok_or_else(|| {
                    CodegenError::Internal(format!("Unknown index local: {:?}", local))
                })?;
                let idx_val = builder.use_var(*idx_var);
                let base = Self::last_collection_base(builder, place, addr, type_ctx);
                Self::translate_collection_index_write(
                    builder,
                    ctx,
                    base,
                    idx_val,
                    value,
                    current_type,
                    type_ctx,
                )?;
            }
        }
        Ok(())
    }

    /// [`Self::collection_base`] for the last projection of `place`.
    fn last_collection_base(
        builder: &mut FunctionBuilder,
        place: &Place,
        value: Value,
        type_ctx: &TypeCtx,
    ) -> CollectionBase {
        let position = place.projection.len().saturating_sub(1);
        Self::collection_base(builder, place, position, value, type_ctx)
    }

    /// Look up a cached `FuncId` for `name`, or declare it as a runtime import
    /// when missing. Used by helpers that need the `FuncId` itself (not just an
    /// instruction emit) — e.g. taking the address of a runtime function for
    /// passing as a callback.
    fn get_or_declare_runtime_func(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &'static str,
        param_types: &[cranelift_codegen::ir::Type],
        return_types: &[cranelift_codegen::ir::Type],
    ) -> Result<cranelift_module::FuncId, CodegenError> {
        if let Some(&id) = ctx.cached_funcs.get(name) {
            return Ok(id);
        }
        let sig = Signature {
            params: param_types.iter().map(|&t| AbiParam::new(t)).collect(),
            returns: return_types.iter().map(|&t| AbiParam::new(t)).collect(),
            call_conv: builder.func.signature.call_conv,
        };
        let id = ctx
            .module
            .declare_function(name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;
        ctx.cached_funcs.insert(name, id);
        Ok(id)
    }

    /// Declare-and-cache a runtime function, then call it.
    ///
    /// `module` and `cached_funcs` are passed separately to avoid double-
    /// borrowing `ModuleCtx`. The cache is keyed by the symbol name and
    /// populates lazily on first use.
    pub(crate) fn call_cached_func(
        builder: &mut FunctionBuilder,
        module: &mut ObjectModule,
        cached_funcs: &mut HashMap<&'static str, cranelift_module::FuncId>,
        call: CallSite,
    ) -> Result<cranelift_codegen::ir::Inst, CodegenError> {
        let CallSite {
            name,
            param_types,
            return_types,
            args,
        } = call;
        let func_id = if let Some(&id) = cached_funcs.get(name) {
            id
        } else {
            let sig = Signature {
                params: param_types.iter().map(|&t| AbiParam::new(t)).collect(),
                returns: return_types.iter().map(|&t| AbiParam::new(t)).collect(),
                call_conv: builder.func.signature.call_conv,
            };
            let id = module
                .declare_function(name, Linkage::Import, &sig)
                .map_err(|e| CodegenError::declare_function(name, e.to_string()))?;
            cached_funcs.insert(name, id);
            id
        };
        let local_func = module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().call(local_func, args))
    }

    pub(crate) fn call_rt_array_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_count: Value,
        elem_size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(elem_count);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_NEW,
                param_types: &[pt, pt],
                return_types: &[pt],
                args: &[elem_count, elem_size],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_array_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_list_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_map_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        key_size: Value,
        value_size: Value,
        key_kind: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(key_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_NEW,
                param_types: &[pt, pt, pt],
                return_types: &[pt],
                args: &[key_size, value_size, key_kind],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_map_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Returns the address of `miri_rt_list_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_list_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::LIST_DECREF_ELEMENT, ptr_type)
    }

    /// Look up (or declare) a `miri_rt_*_decref_element(ptr)` runtime helper
    /// and return its function address as a ptr-sized integer.
    fn decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &'static str,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let func_id = Self::get_or_declare_runtime_func(builder, ctx, name, &[ptr_type], &[])?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_map_set_val_drop_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_val_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_VAL_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_map_set_key_drop_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_key_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_KEY_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_map_set_key_kind(map_ptr, key_kind)`.
    pub(crate) fn call_rt_map_set_key_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        key_kind: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_KEY_KIND,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, key_kind],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_map_set_key_hash_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_key_hash_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::MAP_SET_KEY_HASH_FN, map_ptr, fn_ptr)
    }

    /// Calls `miri_rt_map_set_key_equals_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_key_equals_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::MAP_SET_KEY_EQUALS_FN, map_ptr, fn_ptr)
    }

    /// Calls `miri_rt_set_set_elem_kind(set_ptr, kind)`.
    pub(crate) fn call_rt_set_set_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        kind: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::SET_SET_ELEM_KIND, set_ptr, kind)
    }

    /// Calls `miri_rt_set_set_elem_hash_fn(set_ptr, fn_ptr)`.
    pub(crate) fn call_rt_set_set_elem_hash_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::SET_SET_ELEM_HASH_FN, set_ptr, fn_ptr)
    }

    /// Calls `miri_rt_set_set_elem_equals_fn(set_ptr, fn_ptr)`.
    pub(crate) fn call_rt_set_set_elem_equals_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::SET_SET_ELEM_EQUALS_FN, set_ptr, fn_ptr)
    }

    /// Calls a runtime `name(container_ptr, value)` that records one
    /// pointer-sized setting on a container and returns nothing.
    fn call_rt_container_setter(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &'static str,
        container_ptr: Value,
        value: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(container_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[container_ptr, value],
            },
        )?;
        Ok(())
    }

    /// Returns the address of `miri_rt_string_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_string_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::STRING_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `miri_rt_array_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_array_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::ARRAY_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `miri_rt_set_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_set_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::SET_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of `miri_rt_map_decref_element` as a ptr-sized integer.
    pub(crate) fn get_rt_map_decref_element_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        Self::decref_element_addr(builder, ctx, rt::MAP_DECREF_ELEMENT, ptr_type)
    }

    /// Returns the address of the decref thunk `thunk` as a ptr-sized integer.
    ///
    /// Used as `elem_drop_fn` for List/Set/Map holding custom-type elements.
    /// The function is generated by `generate_drop_function` with Export linkage
    /// so declaring it here as Import allows the linker to resolve it.
    pub(crate) fn get_custom_decref_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        thunk: &Symbol,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let decref_name = thunk.link_name();
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type)],
            returns: vec![],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&decref_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_list_set_elem_drop_fn(list_ptr, fn_ptr)`.
    pub(crate) fn call_rt_list_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_SET_ELEM_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[list_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_array_set_elem_drop_fn(array_ptr, fn_ptr)`.
    pub(crate) fn call_rt_array_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        array_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(array_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_SET_ELEM_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[array_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Returns the address of the clone thunk `thunk` as a ptr-sized integer.
    ///
    /// Used as `elem_clone_fn` for Array/List/Set holding custom-type elements.
    /// The function is generated by `generate_clone_function` with Export linkage.
    pub(crate) fn get_custom_clone_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        thunk: &Symbol,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let clone_name = thunk.link_name();
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type)],
            returns: vec![AbiParam::new(ptr_type)],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&clone_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(clone_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_array_set_elem_clone_fn(array_ptr, fn_ptr)`.
    pub(crate) fn call_rt_array_set_elem_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        array_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(array_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_SET_ELEM_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[array_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_list_set_elem_clone_fn(list_ptr, fn_ptr)`.
    pub(crate) fn call_rt_list_set_elem_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_SET_ELEM_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[list_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Returns the address of the compare thunk `thunk` as a ptr-sized integer.
    ///
    /// Used as `elem_compare_fn` for List/Array holding elements whose bytes are
    /// a reference. The function is generated by `generate_element_method_thunk`
    /// with Export linkage.
    pub(crate) fn get_custom_compare_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        thunk: &Symbol,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let compare_name = thunk.link_name();
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type), AbiParam::new(ptr_type)],
            returns: vec![AbiParam::new(ptr_type)],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&compare_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(compare_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Returns the address of the equality thunk `thunk` as a ptr-sized integer.
    ///
    /// Used as the element equality of a Set, or the key equality of a Map,
    /// holding elements whose type defines `equals`. The function is generated
    /// by `generate_element_method_thunk` with Export linkage.
    /// The address of the hash thunk `thunk`, `(value) -> int`, which the
    /// pipeline lowered for the element type a container hashes through it.
    pub(crate) fn get_element_hash_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        thunk: &Symbol,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let hash_name = thunk.link_name();
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type)],
            returns: vec![AbiParam::new(cranelift_codegen::ir::types::I64)],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&hash_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(hash_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    pub(crate) fn get_custom_equals_thunk_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        thunk: &Symbol,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<Value, CodegenError> {
        let equals_name = thunk.link_name();
        let sig = Signature {
            params: vec![AbiParam::new(ptr_type), AbiParam::new(ptr_type)],
            returns: vec![AbiParam::new(cranelift_codegen::ir::types::I8)],
            call_conv: builder.func.signature.call_conv,
        };
        let func_id = ctx
            .module
            .declare_function(&equals_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(equals_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        Ok(builder.ins().func_addr(ptr_type, local_func))
    }

    /// Calls `miri_rt_list_set_elem_compare_fn(list_ptr, fn_ptr)`.
    pub(crate) fn call_rt_list_set_elem_compare_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(list_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::LIST_SET_ELEM_COMPARE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[list_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_array_set_elem_compare_fn(array_ptr, fn_ptr)`.
    pub(crate) fn call_rt_array_set_elem_compare_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        array_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(array_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_SET_ELEM_COMPARE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[array_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_list_set_elem_order_kind(list_ptr, kind)`.
    pub(crate) fn call_rt_list_set_elem_order_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        list_ptr: Value,
        kind: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::LIST_SET_ELEM_ORDER_KIND, list_ptr, kind)
    }

    /// Calls `miri_rt_array_set_elem_order_kind(array_ptr, kind)`.
    pub(crate) fn call_rt_array_set_elem_order_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        array_ptr: Value,
        kind: Value,
    ) -> Result<(), CodegenError> {
        Self::call_rt_container_setter(builder, ctx, rt::ARRAY_SET_ELEM_ORDER_KIND, array_ptr, kind)
    }

    /// Calls `miri_rt_set_set_elem_clone_fn(set_ptr, fn_ptr)`.
    pub(crate) fn call_rt_set_set_elem_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(set_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_SET_ELEM_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[set_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_map_set_val_clone_fn(map_ptr, fn_ptr)`.
    pub(crate) fn call_rt_map_set_val_clone_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        map_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(map_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::MAP_SET_VAL_CLONE_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[map_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_set_new(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(elem_size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_NEW,
                param_types: &[pt],
                return_types: &[pt],
                args: &[elem_size],
            },
        )?;
        Ok(builder.inst_results(inst)[0])
    }

    pub(crate) fn call_rt_set_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_set_set_elem_drop_fn(set_ptr, fn_ptr)`.
    pub(crate) fn call_rt_set_set_elem_drop_fn(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        set_ptr: Value,
        fn_ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(set_ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::SET_SET_ELEM_DROP_FN,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[set_ptr, fn_ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_string_free(ptr)`.
    ///
    /// `ptr` must be the payload pointer (past the RC header) returned by a
    /// `miri_rt_string_*` constructor.  The runtime function runs the
    /// `MiriString` destructor (freeing the inner data buffer) and then frees
    /// the `[RC][payload]` block via `free_with_rc`.
    pub(crate) fn call_rt_string_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::STRING_FREE,
                param_types: &[pt],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_closure_alloc_track()` to record a closure allocation in
    /// `CLOSURE_ALLOC_BALANCE`. Must be matched by `call_rt_closure_free_track`.
    pub(crate) fn call_rt_closure_alloc_track(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
    ) -> Result<(), CodegenError> {
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CLOSURE_ALLOC_TRACK,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_closure_free_track()` to record a closure free in
    /// `CLOSURE_ALLOC_BALANCE`. Must follow a matching `call_rt_closure_alloc_track`.
    pub(crate) fn call_rt_closure_free_track(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
    ) -> Result<(), CodegenError> {
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CLOSURE_FREE_TRACK,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
        )?;
        Ok(())
    }

    pub(crate) fn call_rt_array_panic_oob(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        index: Value,
        len: Value,
    ) -> Result<(), CodegenError> {
        let pt = builder.func.dfg.value_type(index);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::ARRAY_PANIC_OOB,
                param_types: &[pt, pt],
                return_types: &[],
                args: &[index, len],
            },
        )?;
        Ok(())
    }

    /// Translates an index access on a collection (Array or List).
    ///
    /// Both `MiriArray` and `MiriList` have compatible layout for the first two fields:
    /// - offset 0: `data: *mut u8` (pointer to element storage)
    /// - offset ptr_size: `elem_count/len: usize`
    ///
    /// This method:
    /// 1. Reads the data pointer from the struct
    /// 2. Reads the length for bounds checking
    /// 3. Emits a trap on out-of-bounds
    /// 4. Computes the element address: `data + index * elem_size`
    pub(crate) fn translate_collection_index_read(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base: CollectionBase,
        idx_val: Value,
        base_type: &Type,
        type_ctx: &TypeCtx,
    ) -> Result<Value, CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i32;
        if let Some(layout) = type_ctx.value_layout(&base_type.kind) {
            let base_value = base.address(base_type)?;
            return Self::read_value_tuple_element(builder, ctx, base_value, idx_val, &layout);
        }

        // No element type ⇒ assume pointer-sized addressing (managed collections
        // hold pointers; this matches the historical silent fallback but is now
        // explicit at the call site rather than inside the resolver).
        let elem_kind = Self::resolve_collection_elem_type(base_type);
        // Inline elements — a vector at its std430 stride, a value held as its
        // bytes at its own — are addressed by their stride and the index
        // expression yields the element *address* (the projection chain loads
        // individual fields from it); pointer-sized elements are loaded.
        let inline = elem_kind.and_then(|k| inline_stride(k, type_ctx));
        let cl_elem_ty = match elem_kind {
            Some(k) => crate::codegen::cranelift::types::translate_type_kind(k, ptr_type),
            None => ptr_type,
        };
        let elem_size = inline.unwrap_or(cl_elem_ty.bytes() as i64);

        // Tuples store `[count][field0][field1]...` at `base_value`; the
        // `data` field is implicit (fields start right after the count slot).
        // Also covers the post-normalization `Custom(TUPLE_TYPE_NAME, ...)`
        // form produced for homogeneous tuples by the type checker.
        let is_tuple_type = base_type.kind.is_tuple();

        let (len_val, fields_base) = if is_tuple_type {
            let base_value = base.address(base_type)?;
            let len = builder.ins().load(ptr_type, MemFlags::new(), base_value, 0);
            let fields = builder.ins().iadd_imm(base_value, ptr_size as i64);
            (len, fields)
        } else {
            let stride = inline.map(|_| elem_size);
            let header = Self::element_header_of(builder, base, base_type, stride, ptr_type);
            (header.len, header.data)
        };
        let fields = ElementHeader {
            data: fields_base,
            len: len_val,
        };
        let elem_addr = Self::element_address(builder, ctx, base, fields, idx_val, elem_size)?;
        // Inline element: the projection chain consumes the address directly, so
        // it stays valid only while the collection holds the slot. A caller that
        // keeps the element past that has to copy its components out first.
        if inline.is_some() {
            return Ok(elem_addr);
        }
        Ok(builder
            .ins()
            .load(cl_elem_ty, MemFlags::new(), elem_addr, 0))
    }

    /// Where an index projection at `position` of `place`, reached at
    /// `value`, finds its collection's header: read before the loop when the
    /// projection is the first of a place rooted at a collection recorded
    /// stable over the block being translated, at `value` otherwise.
    pub(crate) fn collection_base(
        builder: &mut FunctionBuilder,
        place: &Place,
        position: usize,
        value: Value,
        type_ctx: &TypeCtx,
    ) -> CollectionBase {
        let hoisted = (position == 0)
            .then(|| type_ctx.stable_headers.hoisted(builder, place.local))
            .flatten();
        let Some(header) = hoisted else {
            return CollectionBase::Address(value);
        };
        let is_proven = matches!(
            place.projection.first(),
            Some(PlaceElem::Index(index)) if type_ctx.stable_headers.is_proven(place.local, *index)
        );
        if is_proven {
            CollectionBase::Proven(header)
        } else {
            CollectionBase::Hoisted(header)
        }
    }

    /// The data pointer and length an index into `base` reads: the ones
    /// read before the loop, or loaded now with the slot-size check.
    fn element_header_of(
        builder: &mut FunctionBuilder,
        base: CollectionBase,
        base_type: &Type,
        stride: Option<i64>,
        ptr_type: cl_types::Type,
    ) -> ElementHeader {
        match base {
            CollectionBase::Address(address) => {
                Self::load_element_header(builder, address, base_type, stride, ptr_type)
            }
            CollectionBase::Hoisted(header) | CollectionBase::Proven(header) => header,
        }
    }

    /// Read element `index` of the tuple held as its bytes at `base`: a
    /// scalar is loaded, a whole value is its address.
    fn read_value_tuple_element(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base: Value,
        index: Value,
        layout: &crate::mir::type_facts::ValueLayout,
    ) -> Result<Value, CodegenError> {
        use crate::mir::type_facts::FieldRepr;
        let (address, repr) = Self::value_element_address(builder, ctx, base, index, layout)?;
        Ok(match repr {
            FieldRepr::Scalar(scalar) => builder.ins().load(
                crate::codegen::cranelift::value_locals::scalar_type(*scalar),
                MemFlags::new(),
                address,
                0,
            ),
            FieldRepr::Value(_) | FieldRepr::OptionTag => address,
        })
    }

    /// The address of element `index` of the collection `header` describes,
    /// checked against its length unless `base` carries a proof the index is
    /// in bounds.
    fn element_address(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base: CollectionBase,
        header: ElementHeader,
        index: Value,
        elem_size: i64,
    ) -> Result<Value, CodegenError> {
        let ptr_type = builder.func.dfg.value_type(header.data);
        if base.needs_bounds_check() {
            return Self::bounds_checked_elem_addr(
                builder,
                ctx,
                header.data,
                index,
                header.len,
                elem_size,
                ptr_type,
            );
        }
        let byte_offset = builder.ins().imul_imm(index, elem_size);
        Ok(builder.ins().iadd(header.data, byte_offset))
    }

    /// Emit a runtime bounds-check (`idx_val < len_val`) that traps on failure
    /// and otherwise computes `fields_base + idx_val * elem_size`. Both the
    /// panic and continuation blocks are sealed before returning.
    fn bounds_checked_elem_addr(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        fields_base: Value,
        idx_val: Value,
        len_val: Value,
        elem_size: i64,
        ptr_type: cl_types::Type,
    ) -> Result<Value, CodegenError> {
        let oob = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedGreaterThanOrEqual,
            idx_val,
            len_val,
        );
        let panic_block = builder.create_block();
        let cont_block = builder.create_block();
        builder.ins().brif(oob, panic_block, &[], cont_block, &[]);

        builder.switch_to_block(panic_block);
        Self::call_rt_array_panic_oob(builder, ctx, idx_val, len_val)?;
        builder.ins().trap(TrapCode::unwrap_user(1));

        builder.switch_to_block(cont_block);
        let elem_size_val = builder.ins().iconst(ptr_type, elem_size);
        let byte_offset = builder.ins().imul(idx_val, elem_size_val);
        let elem_addr = builder.ins().iadd(fields_base, byte_offset);

        builder.seal_block(panic_block);
        builder.seal_block(cont_block);
        Ok(elem_addr)
    }

    /// Translates an index write on a collection (Array or List).
    /// Same layout assumptions as `translate_collection_index_read`.
    pub(crate) fn translate_collection_index_write(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        base: CollectionBase,
        idx_val: Value,
        value: Value,
        base_type: &Type,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;

        // No element type ⇒ assume pointer-sized addressing (managed collections
        // hold pointers; this matches the historical silent fallback but is now
        // explicit at the call site rather than inside the resolver).
        let elem_type_kind = Self::resolve_collection_elem_type(base_type);
        // Inline elements — a vector at its std430 stride, a value held as its
        // bytes at its own — are written as raw bytes copied from `value` (the
        // source element/aggregate address); they carry no managed pointer to
        // DecRef.
        let inline = elem_type_kind.and_then(|k| inline_stride(k, type_ctx));
        let cl_elem_ty = match elem_type_kind {
            Some(elem_kind) => {
                crate::codegen::cranelift::types::translate_type_kind(elem_kind, ptr_type)
            }
            None => ptr_type,
        };
        let elem_size = inline.unwrap_or(cl_elem_ty.bytes() as i64);

        let stride = inline.map(|_| elem_size);
        let header = Self::element_header_of(builder, base, base_type, stride, ptr_type);
        let elem_addr = Self::element_address(builder, ctx, base, header, idx_val, elem_size)?;
        match (inline, elem_type_kind) {
            (Some(_), Some(elem_kind)) => {
                // Copy the element's bytes inline; `value` is the source
                // element/aggregate address. No DecRef: neither a vector nor
                // a value held as its bytes holds a reference.
                Self::copy_inline_element(builder, value, elem_addr, elem_kind, type_ctx)?;
            }
            (Some(_), None) | (None, None) => {
                builder.ins().store(MemFlags::new(), value, elem_addr, 0);
            }
            (None, Some(elem_kind)) => {
                // DecRef the old managed element before overwriting.
                Self::emit_managed_elem_decref(
                    builder, ctx, elem_addr, elem_kind, ptr_type, type_ctx,
                )?;
                builder.ins().store(MemFlags::new(), value, elem_addr, 0);
            }
        }
        Ok(())
    }

    /// Copy the bytes of an inline element of `kind` from `source` into the
    /// collection slot at `dest`: a vector component by component, a value
    /// held as its bytes as those bytes.
    pub(crate) fn copy_inline_element(
        builder: &mut FunctionBuilder,
        source: Value,
        dest: Value,
        kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if let Some((_, dim, component)) = inline_vec_element_layout(kind, type_ctx) {
            let component_bytes = component.bytes() as i32;
            let lanes = (0..i32::from(dim)).map(|lane| (lane * component_bytes, component));
            copy_chunks(builder, source, dest, lanes);
            return Ok(());
        }
        let layout = type_ctx.value_layout(kind).ok_or_else(|| {
            CodegenError::Internal(format!(
                "`{kind}` is copied as an inline element but is none"
            ))
        })?;
        copy_value(builder, source, dest, &layout);
        Ok(())
    }

    /// Allocates a `size`-byte block through the runtime, caching the FuncId
    /// across invocations. The runtime records the size in the block's first
    /// word; every other byte is unspecified, so the caller stores all of them
    /// it reads.
    ///
    /// Every inline heap allocation compiled code makes — class instances,
    /// tuples, `Option`s, enum payloads, closure environments — funnels through
    /// here, so registering the result with the heap guard at this one point
    /// covers them all and cannot be missed by a future caller.
    pub(crate) fn call_rt_block_alloc(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        size: Value,
    ) -> Result<Value, CodegenError> {
        let pt = builder.func.dfg.value_type(size);
        let inst = Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::BLOCK_ALLOC,
                param_types: &[pt],
                return_types: &[pt],
                args: &[size],
            },
        )?;
        let raw_ptr = builder.inst_results(inst)[0];
        // Emitted before the caller's own null check, so the runtime hook must
        // treat a null pointer as a no-op.
        Self::emit_tracked(builder, ctx, |builder, ctx| {
            Self::call_rt_class_alloc_track(builder, ctx, raw_ptr)
        })?;
        Ok(raw_ptr)
    }

    /// Emit `body` under a test of the runtime's tracking state, so it runs only
    /// while the runtime asks to observe allocations.
    ///
    /// The hook `body` calls crosses into the runtime static library, which a
    /// Cranelift-emitted program cannot inline, so an unobserved allocation
    /// would otherwise pay a full call to learn there is nothing to report.
    /// Testing a runtime byte here costs a load and a branch that predicts.
    ///
    /// The state starts out neither on nor off, and only the value meaning
    /// "off" skips: the first allocation therefore still calls in, and that call
    /// is what settles the state. Nothing is missed before the runtime has read
    /// its environment, which matters because it has no startup hook to read it
    /// in.
    fn emit_tracked(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        body: impl FnOnce(&mut FunctionBuilder, &mut ModuleCtx) -> Result<(), CodegenError>,
    ) -> Result<(), CodegenError> {
        if ctx.runtime_checks == RuntimeChecks::CompiledOut {
            return Ok(());
        }
        let is_off = Self::emit_tracking_is_off(builder, ctx)?;
        let track_block = builder.create_block();
        let merge_block = builder.create_block();
        builder
            .ins()
            .brif(is_off, merge_block, &[], track_block, &[]);

        builder.switch_to_block(track_block);
        body(builder, ctx)?;
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(merge_block);
        Ok(())
    }

    /// Loads the runtime's tracking-state byte and answers whether it reads
    /// "off": no heap guard, leak check or counter is observing the program.
    pub(crate) fn emit_tracking_is_off(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
    ) -> Result<Value, CodegenError> {
        use cranelift_module::Module;

        let state_id = ctx
            .module
            .declare_data(rt::TRACKING_STATE, Linkage::Import, false, false)
            .map_err(|e| {
                CodegenError::declare_function(rt::TRACKING_STATE.to_string(), e.to_string())
            })?;
        let state_gv = ctx.module.declare_data_in_func(state_id, builder.func);
        let ptr_type = ctx.module.target_config().pointer_type();
        let state_addr = builder.ins().symbol_value(ptr_type, state_gv);
        let state = builder
            .ins()
            .load(cl_types::I8, MemFlags::new(), state_addr, 0);
        Ok(builder.ins().icmp_imm(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            state,
            rt::TRACKING_STATE_OFF,
        ))
    }

    /// Calls `miri_rt_checks_compiled_out()`, which tells the runtime this
    /// program carries none of its checking hooks.
    fn call_rt_checks_compiled_out(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
    ) -> Result<(), CodegenError> {
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CHECKS_COMPILED_OUT,
                param_types: &[],
                return_types: &[],
                args: &[],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_profile_init(table, count)`, handing the runtime's
    /// sampler the profile build's function table.
    fn call_rt_profile_init(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        table: ProfileTable,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let global = ctx.module.declare_data_in_func(table.data, builder.func);
        let address = builder.ins().symbol_value(ptr_type, global);
        let count = builder.ins().iconst(ptr_type, table.count as i64);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::PROFILE_INIT,
                param_types: &[ptr_type, ptr_type],
                return_types: &[],
                args: &[address, count],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_class_alloc_track(ptr)` to register an inline allocation
    /// with the heap guard.
    pub(crate) fn call_rt_class_alloc_track(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let ptr_type = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CLASS_ALLOC_TRACK,
                param_types: &[ptr_type],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Calls `miri_rt_release_check(ptr)`, while the runtime is tracking, so the
    /// heap guard sees a release of `ptr` before compiled code reads the
    /// reference count below it — which, on a block already freed, is a read
    /// of freed memory.
    pub(crate) fn emit_release_check(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let ptr_type = builder.func.dfg.value_type(ptr);
        Self::emit_tracked(builder, ctx, |builder, ctx| {
            Self::call_cached_func(
                builder,
                ctx.module,
                &mut ctx.cached_funcs,
                CallSite {
                    name: rt::RELEASE_CHECK,
                    param_types: &[ptr_type],
                    return_types: &[],
                    args: &[ptr],
                },
            )?;
            Ok(())
        })
    }

    /// Calls the runtime hook `name`, which takes nothing and returns nothing,
    /// while the runtime asks to observe: the `MIRI_RC_STATS` counters.
    pub(crate) fn emit_rc_stats_hook(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &'static str,
    ) -> Result<(), CodegenError> {
        Self::emit_tracked(builder, ctx, |builder, ctx| {
            Self::call_cached_func(
                builder,
                ctx.module,
                &mut ctx.cached_funcs,
                CallSite {
                    name,
                    param_types: &[],
                    return_types: &[],
                    args: &[],
                },
            )?;
            Ok(())
        })
    }

    /// Calls `miri_rt_class_free_track(ptr)` so the heap guard witnesses an
    /// inline `free` before compiled code performs it.
    pub(crate) fn call_rt_class_free_track(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let ptr_type = builder.func.dfg.value_type(ptr);
        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::CLASS_FREE_TRACK,
                param_types: &[ptr_type],
                return_types: &[],
                args: &[ptr],
            },
        )?;
        Ok(())
    }

    /// Releases an inline block through the runtime, caching the FuncId
    /// across invocations.
    ///
    /// `header_ptr` points to the RC header (payload - ptr_size); the block
    /// [`Self::call_rt_block_alloc`] returned starts one word before it.
    pub(crate) fn call_rt_block_free(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        header_ptr: Value,
    ) -> Result<(), CodegenError> {
        let ptr_type = builder.func.dfg.value_type(header_ptr);
        let ptr_size = ptr_type.bytes() as i64;
        let base = builder.ins().iadd_imm(header_ptr, -ptr_size);

        // Every inline release funnels through here, so the guard witnesses the
        // free at one point rather than at each `emit_drop_*` arm. It is keyed
        // on the same allocation base the matching allocation reported.
        Self::emit_tracked(builder, ctx, |builder, ctx| {
            Self::call_rt_class_free_track(builder, ctx, base)
        })?;

        Self::call_cached_func(
            builder,
            ctx.module,
            &mut ctx.cached_funcs,
            CallSite {
                name: rt::BLOCK_FREE,
                param_types: &[ptr_type],
                return_types: &[],
                args: &[base],
            },
        )?;
        Ok(())
    }
}

/// One field read along a place: the aggregate holding it, its index, the
/// kind the projection reaches, and — at the place's last projection — the
/// type the destination reads it as.
struct FieldRead<'r> {
    owner: &'r Type,
    idx: usize,
    reached: &'r TypeKind,
    expected: Option<&'r Type>,
}

/// The width in bytes a set or map slot holding an element of type `kind` must
/// have.
///
/// The container is allocated once, from the type its declaration names: the
/// element's stride, never less than a value word. The floor is what lets a
/// method compiled once for every element type hand the container its element
/// in a value word; the runtime lays each element it is handed out at this
/// width (see [`element_layout`] for the element's own width).
pub(crate) fn declared_element_bytes(kind: &TypeKind, type_ctx: &TypeCtx) -> u32 {
    let stride = element_layout(kind, type_ctx.layout_scope()).stride;
    u32::try_from(stride)
        .unwrap_or(u32::MAX)
        .max(type_ctx.ptr_type.bytes())
}

/// The type a collection's type argument names, or `None` when the argument is
/// not a type expression.
pub(crate) fn type_argument(argument: &crate::ast::expression::Expression) -> Option<&Type> {
    match &argument.node {
        crate::ast::expression::ExpressionKind::Type(ty, _) => Some(ty),
        _ => None,
    }
}

/// The stride of an element of `kind` a collection lays out inline — an
/// inline vector or a value held as its bytes — or `None` for an element held
/// in one word, which a read loads.
pub(crate) fn inline_stride(kind: &TypeKind, type_ctx: &TypeCtx) -> Option<i64> {
    if let Some((stride, _, _)) = inline_vec_element_layout(kind, type_ctx) {
        return Some(stride);
    }
    type_ctx.value_layout(kind)?;
    Some(element_layout(kind, type_ctx.layout_scope()).stride)
}

/// Inline layout of a vector value element (`Vec2`/`Vec3`/`Vec4<scalar>`) stored
/// directly in a collection buffer: `(std430 stride, dimension, component
/// Cranelift type)`. Returns `None` for non-vector / pointer-sized elements,
/// which keeps every caller's non-inline path on its existing behavior.
pub(crate) fn inline_vec_element_layout(
    kind: &TypeKind,
    type_ctx: &TypeCtx,
) -> Option<(i64, u8, cl_types::Type)> {
    let TypeKind::Custom(name, Some(args)) = kind else {
        return None;
    };
    let dim = crate::ast::types::vec_dim(name)?;
    let crate::ast::expression::ExpressionKind::Type(scalar, _) = &args.first()?.node else {
        return None;
    };
    let layout = element_layout(kind, type_ctx.layout_scope());
    if !layout.is_address {
        return None;
    }
    let stride = layout.stride;
    let component =
        crate::codegen::cranelift::types::translate_type_kind(&scalar.kind, type_ctx.ptr_type);
    Some((stride, dim, component))
}

/// The Cranelift type the word member `idx` of `owner` is read at; a value
/// held inline there has none, and asking for one is refused.
fn member_word(
    member: &layout::Member,
    owner: &TypeKind,
    idx: usize,
) -> Result<cl_types::Type, CodegenError> {
    member.word().ok_or_else(|| {
        CodegenError::Internal(format!(
            "field {idx} of `{owner}` is a value held inline, read here as a word"
        ))
    })
}
