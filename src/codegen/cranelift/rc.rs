// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference-counting code emission: drop/decref/clone thunk generation and
//! per-type field-walking drop logic. Runtime FFI call wrappers live in
//! `translator.rs`; this module dispatches into them.

use crate::ast::expression::Expression;
use crate::ast::factory::type_expr_non_null;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::codegen::cranelift::layout;
use crate::codegen::cranelift::structural_elements::StructuralElement;
use crate::codegen::cranelift::translator::{
    empty_module_ctx, CallSite, ElementShape, FunctionTranslator, ModuleCtx, TypeCtx,
};
use crate::codegen::cranelift::value_abi::{miri_signature, Passing, Returning};
use crate::codegen::cranelift::value_locals::ValueLocals;
use crate::error::CodegenError;
use crate::mir::option_layout::{resolve_alias, OptionalLayers};
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::type_facts::TypeFacts;
use crate::mir::type_facts::{TypeDefinition, ValueLayout};
use crate::runtime_fns::rt;

use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, MemFlagsData, Signature, Value};
use cranelift_codegen::isa::TargetIsa;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

pub(crate) use crate::mir::type_facts::Release;
use crate::mir::type_facts::{
    release_plan, releases_slot, CountedSlot, DropPlan, DropThunk, DropThunkRefusal,
    DroppedMembers, ElementIdentity, ElementQuestion, FloatWidth, ReleasePlan, UnresolvedArgument,
};

/// A runtime call recording one pointer-sized setting on a container.
pub(crate) type ContainerSetter =
    fn(&mut FunctionBuilder, &mut ModuleCtx, Value, Value) -> Result<(), CodegenError>;

/// The runtime setters through which a set's elements, or a map's keys, learn
/// how two of them are matched. See [`FunctionTranslator::emit_element_identity`].
#[derive(Clone, Copy)]
pub(crate) struct ElementIdentitySetters {
    /// Selects the built-in rule: raw bytes or string content.
    pub(crate) set_kind: ContainerSetter,
    /// Routes matching through a generated `equals` thunk.
    pub(crate) set_equals_fn: ContainerSetter,
    /// Routes hashing through the generated `hash` thunk consistent with it.
    pub(crate) set_hash_fn: ContainerSetter,
}

/// The rule by which a container recognises two of its elements, or a map two
/// of its keys, as the same one.
#[derive(Clone, Copy)]
pub(crate) enum ElementRule {
    /// The bytes of the value are the value.
    Bytes,
    /// The value points at a string, matched by its content.
    StringContent,
    /// The value is a float of this kind, matched by the number it holds.
    FloatValue(i64),
    /// The value is matched through a registered `equals` thunk.
    OwnEquals,
}

/// The runtime setters through which a list's or array's elements learn how
/// two of them are ordered. See [`FunctionTranslator::emit_element_order`].
#[derive(Clone, Copy)]
pub(crate) struct ElementOrderSetters {
    /// Selects how an element's bytes read as its value: signed, unsigned or float.
    pub(crate) set_kind: ContainerSetter,
    /// Routes ordering through a generated `compare` thunk.
    pub(crate) set_compare_fn: ContainerSetter,
}

/// The enum value one variant's drop guard releases fields out of.
#[derive(Clone, Copy)]
struct EnumDropSite<'a> {
    /// The discriminant, read once for every variant's guard.
    disc: Value,
    /// Where each field of the value sits, from [`layout::enum_payload_layout`].
    layout: &'a layout::EnumLayout,
    /// Address of the discriminant slot.
    payload_ptr: Value,
}

impl<'a> FunctionTranslator<'a> {
    /// Address of the runtime decref helper for an element of `shape`, or
    /// `None` when the shape needs no decref (primitives, void, etc.).
    pub(crate) fn elem_decref_addr_for_shape(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        shape: ElementShape,
        ptr_type: cl_types::Type,
    ) -> Result<Option<Value>, CodegenError> {
        let addr = match shape {
            ElementShape::String => {
                Self::get_rt_string_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::List) => {
                Self::get_rt_list_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Array) => {
                Self::get_rt_array_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Set) => {
                Self::get_rt_set_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::Builtin(BuiltinCollectionKind::Map) => {
                Self::get_rt_map_decref_element_addr(builder, ctx, ptr_type)?
            }
            ElementShape::UserClass(name) => {
                let thunk = Symbol::type_thunk(ThunkKind::Decref, name, &[]);
                Self::get_custom_decref_thunk_addr(builder, ctx, &thunk, ptr_type)?
            }
            ElementShape::Other => return Ok(None),
        };
        Ok(Some(addr))
    }

    /// Address of the decref helper to register as a collection's `elem_drop_fn`
    /// for element type `elem_kind`, or `None` when the element needs no decref.
    ///
    /// A recorded generic-class instantiation (`Box<String>`) routes to its
    /// per-instantiation `miri.Box$String.$decref` wrapper so the concrete managed
    /// field is released when the runtime drops an element (`clear`, `remove_at`,
    /// `pop`). A structural element — a tuple, an option, a function value —
    /// routes to the thunk generated for its structure, since it has no
    /// declaration to name. All other shapes — including non-generic classes
    /// and unrecorded instantiations — fall back to the shared per-shape helper.
    pub(crate) fn elem_decref_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        // A value held as its bytes holds no reference to release.
        if type_ctx.value_layout(elem_kind).is_some() {
            return Ok(None);
        }
        if let Some(encoding) =
            crate::codegen::cranelift::structural_elements::structural_thunk_symbol(elem_kind)
        {
            let thunk = Symbol::structural_thunk(ThunkKind::Decref, encoding);
            let addr = Self::get_custom_decref_thunk_addr(builder, ctx, &thunk, ptr_type)?;
            return Ok(Some(addr));
        }
        let shape = Self::classify_element_shape(elem_kind);
        if let ElementShape::UserClass(class_name) = shape {
            // Every caller is expected to have screened the element already. If
            // one has not, refuse here rather than name a release helper for a
            // parameter that has no concrete type yet: that symbol is defined
            // nowhere, so emitting it turns a compile into a link failure
            // reporting a mangled name instead of the program.
            if crate::mir::type_facts::is_unresolved_element(
                elem_kind,
                type_ctx.facts.definitions(),
            ) {
                return Err(CodegenError::Internal(format!(
                    "refusing to register a release helper named for the unresolved generic \
                     parameter '{class_name}': the registration site must skip an element whose \
                     type is not yet concrete, as the others do"
                )));
            }
            let per_instantiation =
                Self::drop_thunk(class_name, Self::custom_type_args(elem_kind), type_ctx)?
                    .per_instantiation();
            let thunk = Symbol::type_thunk(
                ThunkKind::Decref,
                class_name,
                per_instantiation.iter().flatten(),
            );
            let addr = Self::get_custom_decref_thunk_addr(builder, ctx, &thunk, ptr_type)?;
            return Ok(Some(addr));
        }
        Self::elem_decref_addr_for_shape(builder, ctx, shape, ptr_type)
    }

    /// Address of the decref helper to register as a map's `key_drop_fn` for key
    /// type `key_kind`, or `None` when the key is a value type the map stores by
    /// its bytes and never releases.
    ///
    /// A key reaches the same helper a collection element of that type would;
    /// the separate entry point exists to skip a generic parameter that has no
    /// concrete symbol yet, the way the value side already does.
    pub(crate) fn key_decref_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        key_kind: &TypeKind,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(key_kind, type_ctx.facts.definitions()) {
            return Ok(None);
        }
        Self::elem_decref_addr_for_kind(builder, ctx, key_kind, ptr_type, type_ctx)
    }

    /// The type-argument expressions of a `TypeKind::Custom`, or `None` for any
    /// other kind. Used to resolve a collection element's generic instantiation.
    fn custom_type_args(kind: &TypeKind) -> Option<&[Expression]> {
        if let TypeKind::Custom(_, args) = kind {
            args.as_deref()
        } else {
            None
        }
    }

    /// Address of the runtime clone helper for an element of `elem_kind`.
    ///
    /// Only user classes that implement `Cloneable` produce a clone helper;
    /// built-in collections and strings use the runtime's default IncRef path.
    /// A recorded instantiation of a generic class is cloned through the
    /// `clone` compiled for its concrete type, and one whose `clone` lowering
    /// withheld keeps the default path too: nothing that runs copies it.
    pub(crate) fn elem_clone_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        facts: &TypeFacts,
        ptr_type: cl_types::Type,
    ) -> Result<Option<Value>, CodegenError> {
        let ElementShape::UserClass(name) = Self::classify_element_shape(elem_kind) else {
            return Ok(None);
        };
        if !facts.is_cloneable(name) {
            return Ok(None);
        }
        let recorded = facts.element_method_instantiation(name, Self::custom_type_args(elem_kind));
        if facts.is_withheld(&Self::clone_method_symbol(name, recorded.as_deref(), facts)) {
            return Ok(None);
        }
        let thunk = Symbol::type_thunk(ThunkKind::Clone, name, recorded.iter().flatten());
        Ok(Some(Self::get_custom_clone_thunk_addr(
            builder, ctx, &thunk, ptr_type,
        )?))
    }

    /// The `clone` body a clone of a `type_name` element at `inst_args` calls:
    /// the one compiled for that instantiation, or the shared one.
    fn clone_method_symbol(
        type_name: &str,
        inst_args: Option<&[Type]>,
        facts: &TypeFacts,
    ) -> Symbol {
        facts.element_method_symbol(type_name, crate::ast::types::CLONE_METHOD_NAME, inst_args)
    }

    /// Sets `elem_drop_fn` on `list_ptr` based on the declared element type.
    ///
    /// Used when an empty `List<T>()` aggregate is assigned: there are no operands
    /// for `translate_rvalue` to inspect, so the caller provides the element kind
    /// extracted from the assignment target's type annotation.
    ///
    /// `elem_kind` may be a generic placeholder (e.g. `T` inside a generic class
    /// method) — in that case there is no concrete `miri.T.$decref` to register, so
    /// the runtime keeps its default no-op elem_drop_fn.
    pub(crate) fn emit_list_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(elem_kind, type_ctx.facts.definitions()) {
            return Ok(());
        }
        if let Some(addr) =
            Self::elem_decref_addr_for_kind(builder, ctx, elem_kind, type_ctx.ptr_type, type_ctx)?
        {
            Self::call_rt_list_set_elem_drop_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Address of the comparator to register as a container's `elem_compare_fn`
    /// for element type `elem_kind`, or `None` when the element's bytes are its
    /// value and the runtime orders them itself.
    ///
    /// The element type is named the way the collection's decref path names it:
    /// a string through its class, a custom type through its own name, and a
    /// recorded instantiation of a generic class through that instantiation, so
    /// the comparison runs the body compiled for the element's concrete type.
    fn elem_compare_addr_for_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<Option<Value>, CodegenError> {
        let Some(thunk) = type_ctx
            .facts
            .element_method_thunk(ElementQuestion::Compare, elem_kind)
        else {
            return Ok(None);
        };
        Ok(Some(Self::get_custom_compare_thunk_addr(
            builder,
            ctx,
            &thunk,
            type_ctx.ptr_type,
        )?))
    }

    /// Registers how a set recognises two elements, or a map two keys, of
    /// `elem_kind` as the same one: a string by its content, a class through its
    /// own `equals`, anything else by its bytes (the runtime default).
    ///
    /// An optional element is none of those itself — its bytes are the address
    /// of its `Some` box, or the payload pointer itself, null when absent — so
    /// it registers the rule of the value it wraps, together with how many
    /// boxes the runtime must open to reach that value, whether the innermost
    /// layer is such a nullable pointer, and how wide the value is. A rule is
    /// registered only where it is the one `==` applies; an element whose
    /// equality is a walk over fields keeps the byte rule rather than being
    /// matched half-right.
    ///
    /// This is the one place both containers take the rule from. It has to run
    /// before the first element is stored, since the runtime places each element
    /// by the hash of the rule in force when it arrives.
    pub(crate) fn emit_element_identity(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        container_ptr: Value,
        type_ctx: &TypeCtx,
        setters: ElementIdentitySetters,
    ) -> Result<(), CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(elem_kind, type_ctx.facts.definitions()) {
            return Ok(());
        }
        let layers = OptionalLayers::of(elem_kind, type_ctx.layout_scope());
        let value_kind = layers.matched_kind(elem_kind).ok_or_else(|| {
            CodegenError::Internal(format!(
                "a set or map element of `{elem_kind}` boxes an optional held as its bytes, \
                 which no element rule can match"
            ))
        })?;
        // An element held as its bytes whole is matched through its own
        // callbacks; no optional layer is left for the runtime to open.
        let wrapping = (layers.inline == 0).then_some(&layers);
        let identity = type_ctx
            .facts
            .element_identity(value_kind, type_ctx.open_params);
        let rule = match identity {
            ElementIdentity::Unregistered => return Ok(()),
            ElementIdentity::Bytes => ElementRule::Bytes,
            ElementIdentity::StringContent => ElementRule::StringContent,
            ElementIdentity::Float(width) => ElementRule::FloatValue(match width {
                FloatWidth::F16 => Self::F16_VALUE_ELEMENT_KIND,
                FloatWidth::F32 => Self::F32_VALUE_ELEMENT_KIND,
                FloatWidth::F64 => Self::F64_VALUE_ELEMENT_KIND,
            }),
            ElementIdentity::FieldWise => {
                let thunk = |kind| {
                    Self::fieldwise_identity_thunk(kind, value_kind, type_ctx).ok_or_else(|| {
                        CodegenError::Internal(format!(
                            "a set or map element of `{value_kind}` is matched field by field, \
                             but its layout names no structural thunk"
                        ))
                    })
                };
                let (equals, hash) = (thunk(ThunkKind::Equals)?, thunk(ThunkKind::Hash)?);
                Self::register_equality(
                    builder,
                    ctx,
                    container_ptr,
                    &equals,
                    Some(&hash),
                    type_ctx,
                    setters,
                )?
            }
            ElementIdentity::Equals(thunks) => Self::register_equality(
                builder,
                ctx,
                container_ptr,
                &thunks.equals,
                thunks.hash.as_ref(),
                type_ctx,
                setters,
            )?,
        };
        let held = type_ctx.value_layout(value_kind);
        let Some(word) = Self::element_identity_kind(rule, wrapping, held.as_ref(), type_ctx)
        else {
            return Ok(());
        };
        let kind = builder.ins().iconst(type_ctx.ptr_type, word);
        (setters.set_kind)(builder, ctx, container_ptr, kind)
    }

    /// Register the `equals` thunk, and the `hash` thunk consistent with it,
    /// through which a container matches its elements.
    #[allow(clippy::too_many_arguments)]
    fn register_equality(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        container_ptr: Value,
        equals: &Symbol,
        hash: Option<&Symbol>,
        type_ctx: &TypeCtx,
        setters: ElementIdentitySetters,
    ) -> Result<ElementRule, CodegenError> {
        let addr = Self::get_custom_equals_thunk_addr(builder, ctx, equals, type_ctx.ptr_type)?;
        (setters.set_equals_fn)(builder, ctx, container_ptr, addr)?;
        if let Some(hash) = hash {
            let hash = Self::get_element_hash_thunk_addr(builder, ctx, hash, type_ctx.ptr_type)?;
            (setters.set_hash_fn)(builder, ctx, container_ptr, hash)?;
        }
        Ok(ElementRule::OwnEquals)
    }

    /// The field-wise `equals` or `hash` thunk a set or map matches a tuple of
    /// `value_kind` through, when it is held as its bytes and its bytes are
    /// not its value (see [`crate::codegen::cranelift::value_identity_thunks`]).
    fn fieldwise_identity_thunk(
        kind: ThunkKind,
        value_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Option<Symbol> {
        let (encoding, _) = crate::codegen::cranelift::fieldwise_value_identity(
            value_kind,
            type_ctx.layout_scope(),
        )?;
        Some(Symbol::structural_thunk(kind, encoding))
    }

    /// The element kind word registering `rule` for a value wrapped in the
    /// optional layers `wrapping` holds as words — none when the element is
    /// matched whole — or `None` when there is nothing to register: an
    /// unwrapped value matched by its bytes is what every container already
    /// starts at, and an equality callback settles matching on its own.
    ///
    /// A value held as its bytes, laid out as `held`, has no word for a
    /// callback to be handed, so its callbacks are handed its address.
    fn element_identity_kind(
        rule: ElementRule,
        wrapping: Option<&OptionalLayers<'_>>,
        held: Option<&ValueLayout>,
        type_ctx: &TypeCtx,
    ) -> Option<i64> {
        let base = match rule {
            ElementRule::StringContent => Self::STRING_CONTENT_ELEMENT_KIND,
            ElementRule::FloatValue(kind) => kind,
            ElementRule::Bytes | ElementRule::OwnEquals => Self::BYTES_ELEMENT_KIND,
        };
        let base = match held {
            Some(_) => Self::by_slot_address(base),
            None => base,
        };
        let Some(layers) = wrapping.filter(|layers| layers.is_optional()) else {
            return (base != Self::BYTES_ELEMENT_KIND).then_some(base);
        };
        let value_size = match held {
            Some(layout) => layout.size,
            None => crate::codegen::cranelift::types::translate_type_kind(
                &layers.value,
                type_ctx.ptr_type,
            )
            .bytes(),
        };
        Self::optional_element_kind(base, layers, value_size)
    }

    /// Sets `elem_drop_fn` on `set_ptr` based on the declared element type.
    ///
    /// Used when an empty `Set<T>()` aggregate is assigned: there are no operands
    /// for `translate_rvalue` to inspect, so the caller provides the element kind
    /// extracted from the assignment target's type annotation.
    pub(crate) fn emit_set_drop_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        set_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(elem_kind, type_ctx.facts.definitions()) {
            return Ok(());
        }
        if let Some(addr) =
            Self::elem_decref_addr_for_kind(builder, ctx, elem_kind, type_ctx.ptr_type, type_ctx)?
        {
            Self::call_rt_set_set_elem_drop_fn(builder, ctx, set_ptr, addr)?;
        }
        Ok(())
    }

    /// Sets `elem_clone_fn` on `list_ptr` when the element type is a Cloneable
    /// custom class. Mirrors `emit_list_drop_fn_for_elem_kind` but for the clone
    /// side. Called on the empty-constructor path where `translate_rvalue` has no
    /// operands to inspect.
    pub(crate) fn emit_list_clone_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        list_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
        facts: &TypeFacts,
    ) -> Result<(), CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(elem_kind, facts.definitions()) {
            return Ok(());
        }
        if let Some(addr) =
            Self::elem_clone_addr_for_kind(builder, ctx, elem_kind, facts, ptr_type)?
        {
            Self::call_rt_list_set_elem_clone_fn(builder, ctx, list_ptr, addr)?;
        }
        Ok(())
    }

    /// Registers how a list or array orders its elements of `elem_kind`: a
    /// class or string through its own `compare`, an unsigned integer or a
    /// float by the value its bytes hold, anything else by the runtime's signed
    /// reading of those bytes (the default, which registers nothing).
    pub(crate) fn emit_element_order(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        container_ptr: Value,
        type_ctx: &TypeCtx,
        setters: ElementOrderSetters,
    ) -> Result<(), CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(elem_kind, type_ctx.facts.definitions()) {
            return Ok(());
        }
        if let Some(kind) = Self::element_order_kind(elem_kind) {
            let kind = builder.ins().iconst(type_ctx.ptr_type, kind);
            (setters.set_kind)(builder, ctx, container_ptr, kind)?;
        }
        if let Some(addr) = Self::elem_compare_addr_for_kind(builder, ctx, elem_kind, type_ctx)? {
            (setters.set_compare_fn)(builder, ctx, container_ptr, addr)?;
        }
        Ok(())
    }

    /// The order setters of a list, for [`Self::emit_element_order`].
    pub(crate) const LIST_ORDER_SETTERS: ElementOrderSetters = ElementOrderSetters {
        set_kind: Self::call_rt_list_set_elem_order_kind,
        set_compare_fn: Self::call_rt_list_set_elem_compare_fn,
    };

    /// The order setters of an array, for [`Self::emit_element_order`].
    pub(crate) const ARRAY_ORDER_SETTERS: ElementOrderSetters = ElementOrderSetters {
        set_kind: Self::call_rt_array_set_elem_order_kind,
        set_compare_fn: Self::call_rt_array_set_elem_compare_fn,
    };

    /// Sets `elem_clone_fn` on `set_ptr` when the element type is a Cloneable
    /// custom class. Mirrors `emit_set_drop_fn_for_elem_kind` but for the clone
    /// side.
    pub(crate) fn emit_set_clone_fn_for_elem_kind(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_kind: &TypeKind,
        set_ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
        facts: &TypeFacts,
    ) -> Result<(), CodegenError> {
        if crate::mir::type_facts::is_unresolved_element(elem_kind, facts.definitions()) {
            return Ok(());
        }
        if let Some(addr) =
            Self::elem_clone_addr_for_kind(builder, ctx, elem_kind, facts, ptr_type)?
        {
            Self::call_rt_set_set_elem_clone_fn(builder, ctx, set_ptr, addr)?;
        }
        Ok(())
    }

    /// Emits a loop that DecRefs each managed value in a map's hash table.
    ///
    /// Iterates over all `capacity` slots, checks the state byte for the
    /// occupied bit, and DecRefs the value pointer in each occupied slot.
    ///
    /// MiriMap states array: 1 byte per slot — 0 empty, 2 tombstone, and an
    /// occupied slot with its high bit (0x80) set over seven bits of its key's
    /// hash.
    /// Values are stored as pointer-sized entries (managed types are always pointers).
    fn emit_map_managed_values_drop_loop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        states: Value,
        values: Value,
        capacity: Value,
        val_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let loop_header = builder.create_block();
        builder.append_block_param(loop_header, ptr_type);
        let check_block = builder.create_block();
        let decref_block = builder.create_block();
        let increment_block = builder.create_block();
        let after_loop = builder.create_block();

        // Enter loop with index 0
        let zero = builder.ins().iconst(ptr_type, 0);
        let zero_arg = cranelift_codegen::ir::BlockArg::Value(zero);
        builder.ins().jump(loop_header, &[zero_arg]);

        // Loop header: check i < capacity
        builder.switch_to_block(loop_header);
        let i = builder.block_params(loop_header)[0];
        let in_range = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            i,
            capacity,
        );
        builder
            .ins()
            .brif(in_range, check_block, &[], after_loop, &[]);

        // Check the occupied bit of state[i]
        builder.switch_to_block(check_block);
        builder.seal_block(check_block);
        let state_addr = builder.ins().iadd(states, i);
        let state_i8 = builder
            .ins()
            .load(cl_types::I8, MemFlagsData::new(), state_addr, 0);
        let state = builder.ins().uextend(ptr_type, state_i8);
        let occupied_bit = builder.ins().band_imm_u(state, 0x80);
        let is_occupied = builder.ins().icmp_imm_s(
            cranelift_codegen::ir::condcodes::IntCC::NotEqual,
            occupied_bit,
            0,
        );
        builder
            .ins()
            .brif(is_occupied, decref_block, &[], increment_block, &[]);

        // DecRef the value in this slot
        builder.switch_to_block(decref_block);
        builder.seal_block(decref_block);
        let byte_offset = builder.ins().imul_imm_s(i, ptr_size);
        let val_addr = builder.ins().iadd(values, byte_offset);
        let val_ptr = builder
            .ins()
            .load(ptr_type, MemFlagsData::new(), val_addr, 0);
        Self::emit_decref_value(builder, ctx, val_kind, val_ptr, type_ctx)?;
        builder.ins().jump(increment_block, &[]);

        // Increment i and loop back
        builder.switch_to_block(increment_block);
        builder.seal_block(increment_block);
        let next_i = builder.ins().iadd_imm_s(i, 1);
        let next_i_arg = cranelift_codegen::ir::BlockArg::Value(next_i);
        builder.ins().jump(loop_header, &[next_i_arg]);

        builder.seal_block(loop_header);
        builder.seal_block(after_loop);
        builder.switch_to_block(after_loop);
        Ok(())
    }

    /// Emits a loop that DecRefs each managed element in a contiguous pointer array.
    ///
    /// Used when freeing a List or Array whose inner type is managed, so that the
    /// RC of each stored element is properly decremented before the container is freed.
    ///
    /// `data` — pointer to the element buffer (pointer-sized elements assumed).
    /// `len`  — number of elements.
    fn emit_managed_elements_drop_loop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        data: Value,
        len: Value,
        inner_kind: &TypeKind,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        let loop_header = builder.create_block();
        builder.append_block_param(loop_header, ptr_type);
        let loop_body = builder.create_block();
        let after_loop = builder.create_block();

        // Jump into the loop with initial index = 0
        let zero = builder.ins().iconst(ptr_type, 0);
        let zero_arg = cranelift_codegen::ir::BlockArg::Value(zero);
        builder.ins().jump(loop_header, &[zero_arg]);

        builder.switch_to_block(loop_header);
        let i = builder.block_params(loop_header)[0];
        let in_range = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::UnsignedLessThan,
            i,
            len,
        );
        builder
            .ins()
            .brif(in_range, loop_body, &[], after_loop, &[]);

        // loop_body: load element, DecRef it, increment index
        builder.switch_to_block(loop_body);
        builder.seal_block(loop_body); // only predecessor: loop_header

        let byte_offset = builder.ins().imul_imm_s(i, ptr_size);
        let elem_addr = builder.ins().iadd(data, byte_offset);
        let elem_ptr = builder
            .ins()
            .load(ptr_type, MemFlagsData::new(), elem_addr, 0);
        Self::emit_decref_value(builder, ctx, inner_kind, elem_ptr, type_ctx)?;

        let next_i = builder.ins().iadd_imm_s(i, 1);
        let next_i_arg = cranelift_codegen::ir::BlockArg::Value(next_i);
        builder.ins().jump(loop_header, &[next_i_arg]);

        // Seal loop_header now that both predecessors are defined
        builder.seal_block(loop_header);

        builder.seal_block(after_loop); // only predecessor: loop_header
        builder.switch_to_block(after_loop);
        Ok(())
    }

    /// Emits the type-appropriate cleanup when an object's RC reaches zero.
    ///
    /// All heap types share the same `[RC][payload]` layout, so the RC
    /// increment/decrement logic is uniform. The only type-specific part is
    /// *what* to free when RC hits zero:
    /// - Arrays/Lists call their runtime `_free` functions (which handle internal buffers).
    /// - Structs/enums with managed fields: DecRef each managed field, then free the block.
    /// - All other types just need the `[RC][payload]` block freed.
    ///
    /// `ptr` points to the payload (past the RC header).
    /// `header_ptr` points to the RC header (`ptr - ptr_size`).
    pub(crate) fn emit_type_drop(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let plan = release_plan(kind, type_ctx.layout_scope())
            .map_err(|UnresolvedArgument(message)| CodegenError::Internal(message))?;
        // The plan reads an alias as the type it stands for; a tuple's slots
        // and an optional's layout are found the same way.
        let resolved = resolve_alias(kind, type_ctx.facts.definitions());
        let kind = resolved.as_ref().map_or(kind, |target| &target.kind);
        match plan {
            ReleasePlan::Map { counted_value } => {
                Self::emit_drop_map(builder, ctx, counted_value.as_ref(), ptr, type_ctx)
            }
            ReleasePlan::Set => Self::call_rt_set_free(builder, ctx, ptr),
            ReleasePlan::List { counted_element } => Self::emit_drop_list_or_array(
                builder,
                ctx,
                counted_element.as_ref(),
                ptr,
                type_ctx,
                true,
            ),
            ReleasePlan::Array { counted_element } => Self::emit_drop_list_or_array(
                builder,
                ctx,
                counted_element.as_ref(),
                ptr,
                type_ctx,
                false,
            ),
            ReleasePlan::Tuple { counted } => {
                Self::emit_drop_tuple(builder, ctx, kind, &counted, ptr, header_ptr, type_ctx)
            }
            ReleasePlan::Optional {
                payload,
                payload_counted,
            } => {
                // An optional held as its payload's own pointer has no box: the
                // allocation reaching zero is the payload's, released as one.
                if Self::nullable_pointer_layout(kind, type_ctx)? {
                    return Self::emit_type_drop(
                        builder,
                        ctx,
                        &payload.kind,
                        ptr,
                        header_ptr,
                        type_ctx,
                    );
                }
                let counted_payload = payload_counted.then_some(&payload);
                Self::emit_drop_option(builder, ctx, counted_payload, ptr, header_ptr, type_ctx)
            }
            // miri_rt_string_free takes the payload pointer (not header) — it
            // calls free_with_rc internally.
            ReleasePlan::String => Self::call_rt_string_free(builder, ctx, ptr),
            ReleasePlan::Named { name, args } => Self::emit_drop_custom(
                builder,
                ctx,
                &name,
                args.as_deref(),
                ptr,
                header_ptr,
                type_ctx,
            ),
            ReleasePlan::Closure => {
                Self::emit_drop_closure(builder, ctx, ptr, header_ptr, type_ctx)
            }
            ReleasePlan::Block => Self::call_rt_block_free(builder, ctx, header_ptr),
        }
    }

    /// DecRef managed values stored in a `MiriMap`, then free the map struct.
    /// Map layout: `[states][keys][values][len][capacity]...` (ptr-sized fields).
    fn emit_drop_map(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        counted_value: Option<&Type>,
        ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if let Some(val_ty) = counted_value {
            let ptr_type = type_ctx.ptr_type;
            let ptr_size = ptr_type.bytes() as i32;
            let states = builder.ins().load(ptr_type, MemFlagsData::new(), ptr, 0);
            let values = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), ptr, 2 * ptr_size);
            let capacity = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), ptr, 4 * ptr_size);
            Self::emit_map_managed_values_drop_loop(
                builder,
                ctx,
                states,
                values,
                capacity,
                &val_ty.kind,
                type_ctx,
            )?;
        }
        Self::call_rt_map_free(builder, ctx, ptr)
    }

    /// DecRef managed elements before freeing a `MiriList` or `MiriArray`.
    ///
    /// Both layouts begin `[data: ptr][len/elem_count: ptr]...`. For Array, also
    /// zeros `elem_drop_fn` (slot 3 * ptr_size) so the runtime's free path does
    /// not run the per-element decref a second time.
    fn emit_drop_list_or_array(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        counted_element: Option<&Type>,
        ptr: Value,
        type_ctx: &TypeCtx,
        is_list: bool,
    ) -> Result<(), CodegenError> {
        if let Some(inner_ty) = counted_element {
            let ptr_type = type_ctx.ptr_type;
            let ptr_size = ptr_type.bytes() as i32;
            let data = builder.ins().load(ptr_type, MemFlagsData::new(), ptr, 0);
            let len = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), ptr, ptr_size);
            Self::emit_managed_elements_drop_loop(
                builder,
                ctx,
                data,
                len,
                &inner_ty.kind,
                type_ctx,
            )?;
            if !is_list {
                // Array: zero elem_drop_fn (slot 3) so the runtime's free
                // path does not call decref a second time on already-
                // dropped elements.
                let zero = builder.ins().iconst(ptr_type, 0);
                builder
                    .ins()
                    .store(MemFlagsData::new(), zero, ptr, 3 * ptr_size);
            }
        }
        if is_list {
            Self::call_rt_list_free(builder, ctx, ptr)
        } else {
            Self::call_rt_array_free(builder, ctx, ptr)
        }
    }

    /// Release each counted slot of a tuple, then `free()` the RC block.
    /// Layout: `[elem_count: ptr][field0][field1]...` (payload_ptr = `ptr`).
    fn emit_drop_tuple(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        tuple_kind: &TypeKind,
        counted: &[CountedSlot],
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        for slot in counted {
            let (offset, _) =
                layout::field_layout(tuple_kind, slot.index, type_ctx.layout_scope(), ptr_type)?;
            let field_ptr = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), ptr, offset);
            Self::emit_decref_value(builder, ctx, &slot.ty.kind, field_ptr, type_ctx)?;
        }
        Self::call_rt_block_free(builder, ctx, header_ptr)
    }

    /// DecRef a boxed Option's inner value (when managed), then free the box.
    /// An optional held as its payload's pointer never reaches here: it has no
    /// box to free.
    fn emit_drop_option(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        counted_payload: Option<&Type>,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        if let Some(inner) = counted_payload {
            let ptr_type = type_ctx.ptr_type;
            let cl_inner_ty =
                crate::codegen::cranelift::types::translate_type_kind(&inner.kind, ptr_type);
            let inner_ptr = builder.ins().load(cl_inner_ty, MemFlagsData::new(), ptr, 0);
            Self::emit_decref_value(builder, ctx, &inner.kind, inner_ptr, type_ctx)?;
        }
        Self::call_rt_block_free(builder, ctx, header_ptr)
    }

    /// Drop a custom struct/class/enum or trait value.
    ///
    /// A value whose static type another class may stand for — a trait, or a
    /// class something extends — is released as its runtime class, through
    /// the drop slot of the vtable it points at; one pointing at none is of
    /// the static class and takes the static path below.
    fn emit_drop_custom(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        type_args: Option<&[Expression]>,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let release = type_ctx.facts.release(name);
        if release == Release::Exact {
            return Self::emit_drop_custom_static(
                builder, ctx, name, type_args, ptr, header_ptr, type_ctx,
            );
        }
        Self::emit_release_by_runtime_class(builder, ctx, ptr, type_ctx.ptr_type, |builder, ctx| {
            match release {
                Release::Trait => Self::call_rt_block_free(builder, ctx, header_ptr),
                Release::Exact | Release::ByRuntimeClass => Self::emit_drop_custom_static(
                    builder, ctx, name, type_args, ptr, header_ptr, type_ctx,
                ),
            }
        })
    }

    /// Release the instance at `ptr` through the drop slot of the vtable it
    /// points at, or, when its vtable word is null, through `static_release`.
    fn emit_release_by_runtime_class(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
        ptr_type: cl_types::Type,
        static_release: impl FnOnce(&mut FunctionBuilder, &mut ModuleCtx) -> Result<(), CodegenError>,
    ) -> Result<(), CodegenError> {
        let vtable = builder.ins().load(ptr_type, MemFlagsData::new(), ptr, 0);
        let has_vtable_block = builder.create_block();
        let static_block = builder.create_block();
        let done_block = builder.create_block();
        builder
            .ins()
            .brif(vtable, has_vtable_block, &[], static_block, &[]);

        builder.switch_to_block(has_vtable_block);
        builder.seal_block(has_vtable_block);
        let slot_offset = (crate::mir::dispatch::DROP_SLOT * ptr_type.bytes() as usize) as i32;
        let drop_fn = builder
            .ins()
            .load(ptr_type, MemFlagsData::new(), vtable, slot_offset);
        let mut signature = Signature::new(builder.func.signature.call_conv);
        signature.params.push(AbiParam::new(ptr_type));
        let signature_ref = builder.import_signature(signature);
        builder.ins().call_indirect(signature_ref, drop_fn, &[ptr]);
        builder.ins().jump(done_block, &[]);

        builder.switch_to_block(static_block);
        builder.seal_block(static_block);
        static_release(builder, ctx)?;
        builder.ins().jump(done_block, &[]);

        builder.switch_to_block(done_block);
        builder.seal_block(done_block);
        Ok(())
    }

    /// Drop a custom struct/class/enum known to be of exactly `name`.
    ///
    /// A concrete instantiation is released through its own drop thunk
    /// (`miri.Box$String.$drop`), and any other value through its type's
    /// shared thunk when that thunk's plan does anything beyond freeing the
    /// block. Otherwise the block is freed in place, after releasing what the
    /// plan for this value's own arguments names — the counted payloads of a
    /// generic enum held in a shared generic body.
    fn emit_drop_custom_static(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        type_args: Option<&[Expression]>,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let facts = type_ctx.facts;
        let per_instantiation = Self::drop_thunk(name, type_args, type_ctx)?.per_instantiation();
        let needs_thunk = per_instantiation.is_some()
            || facts
                .drop_plan(name, None)
                .is_some_and(DropPlan::releases_anything);
        if needs_thunk {
            let thunk =
                Symbol::type_thunk(ThunkKind::Drop, name, per_instantiation.iter().flatten());
            return Self::call_drop_thunk(builder, ctx, &thunk, ptr, type_ctx.ptr_type);
        }
        if let Some(args) = type_args.and_then(crate::mir::instantiation::type_arguments) {
            let plan = facts.drop_plan_in(name, Some(&args), type_ctx.open_params);
            Self::emit_planned_members(
                builder,
                ctx,
                name,
                Some(&args),
                &plan.members,
                ptr,
                type_ctx,
            )?;
        }
        Self::call_rt_block_free(builder, ctx, header_ptr)
    }

    /// Which drop thunk releases a `name` value carrying `type_args` in the
    /// code being translated, as [`TypeFacts::drop_thunk`] decides it.
    fn drop_thunk(
        name: &str,
        type_args: Option<&[Expression]>,
        type_ctx: &TypeCtx,
    ) -> Result<DropThunk, CodegenError> {
        (type_ctx.facts)
            .drop_thunk(name, type_args, type_ctx.open_params, type_ctx.bound_params)
            .map_err(|DropThunkRefusal(message)| CodegenError::Internal(message))
    }

    /// Drop a closure: invoke its `dtor_ptr` (when non-null) to DecRef captures,
    /// then decrement the closure-balance counter and free the closure struct.
    /// Layout: `payload[0]=fn_ptr, payload[1]=dtor_ptr`, then the captures
    /// (see `CaptureLayout`).
    fn emit_drop_closure(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        ptr: Value,
        header_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;
        let dtor_ptr = builder
            .ins()
            .load(ptr_type, MemFlagsData::new(), ptr, ptr_size as i32);
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            dtor_ptr,
            null,
        );
        let dtor_block = builder.create_block();
        let after_dtor = builder.create_block();
        builder
            .ins()
            .brif(is_null, after_dtor, &[], dtor_block, &[]);
        builder.switch_to_block(dtor_block);
        builder.seal_block(dtor_block);
        let mut dtor_sig = cranelift_codegen::ir::Signature::new(builder.func.signature.call_conv);
        dtor_sig
            .params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        let dtor_sig_ref = builder.import_signature(dtor_sig);
        builder.ins().call_indirect(dtor_sig_ref, dtor_ptr, &[ptr]);
        builder.ins().jump(after_dtor, &[]);
        builder.switch_to_block(after_dtor);
        builder.seal_block(after_dtor);
        Self::call_rt_closure_free_track(builder, ctx)?;
        Self::call_rt_block_free(builder, ctx, header_ptr)
    }

    /// Release the counted members a drop plan names, out of the `name`
    /// value at `payload_ptr` carrying `inst_args`.
    ///
    /// Offsets come from the layout of the instance at its own arguments, so
    /// an inherited field lands at its slot and a field after one a wide
    /// argument binds is found where construction wrote it.
    pub(crate) fn emit_planned_members(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        name: &str,
        inst_args: Option<&[Type]>,
        members: &DroppedMembers,
        payload_ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let arg_exprs: Option<Vec<Expression>> =
            inst_args.map(|args| args.iter().cloned().map(type_expr_non_null).collect());
        match members {
            DroppedMembers::Fields(fields) => {
                let custom_kind = TypeKind::Custom(name.to_string(), arg_exprs);
                for slot in fields {
                    let (offset, _) = layout::field_layout(
                        &custom_kind,
                        slot.index,
                        type_ctx.layout_scope(),
                        ptr_type,
                    )?;
                    let field_ptr =
                        builder
                            .ins()
                            .load(ptr_type, MemFlagsData::new(), payload_ptr, offset);
                    Self::emit_decref_value(builder, ctx, &slot.ty.kind, field_ptr, type_ctx)?;
                }
                Ok(())
            }
            DroppedMembers::Variants(variants) => {
                let Some(TypeDefinition::Enum(enum_def)) = type_ctx.facts.definitions().get(name)
                else {
                    return Err(CodegenError::Internal(format!(
                        "the drop plan of `{name}` releases enum payloads, but `{name}` is no enum"
                    )));
                };
                let disc = builder
                    .ins()
                    .load(ptr_type, MemFlagsData::new(), payload_ptr, 0);
                let enum_layout = layout::enum_payload_layout(
                    enum_def,
                    arg_exprs.as_deref(),
                    type_ctx.layout_scope(),
                    ptr_type,
                );
                for (variant_idx, slots) in variants {
                    let site = EnumDropSite {
                        disc,
                        layout: &enum_layout,
                        payload_ptr,
                    };
                    Self::emit_enum_variant_drop_guard(
                        builder,
                        ctx,
                        site,
                        *variant_idx,
                        slots,
                        type_ctx,
                    )?;
                }
                Ok(())
            }
        }
    }

    /// Emit `if disc == variant_idx { decref each managed field }`. Caller
    /// continues in the merge block after this returns.
    fn emit_enum_variant_drop_guard(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        site: EnumDropSite<'_>,
        variant_idx: usize,
        slots: &[CountedSlot],
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let EnumDropSite {
            disc,
            layout: enum_layout,
            payload_ptr,
        } = site;
        let ptr_type = type_ctx.ptr_type;
        let variant_val = builder.ins().iconst(ptr_type, variant_idx as i64);
        let is_this_variant = builder.ins().icmp(
            cranelift_codegen::ir::condcodes::IntCC::Equal,
            disc,
            variant_val,
        );

        let drop_block = builder.create_block();
        let merge_block = builder.create_block();
        builder
            .ins()
            .brif(is_this_variant, drop_block, &[], merge_block, &[]);

        builder.switch_to_block(drop_block);
        for slot in slots {
            let field_idx = slot.index;
            // Payload field `k` is enum field `k + 1`: the discriminant
            // occupies the first column.
            let field_offset = enum_layout
                .columns
                .get(field_idx + 1)
                .ok_or_else(|| {
                    CodegenError::Internal(format!(
                        "variant {variant_idx} releases payload field {field_idx}, past the \
                         enum's last column"
                    ))
                })?
                .offset;
            let field_ptr =
                builder
                    .ins()
                    .load(ptr_type, MemFlagsData::new(), payload_ptr, field_offset);
            Self::emit_decref_value(builder, ctx, &slot.ty.kind, field_ptr, type_ctx)?;
        }
        builder.ins().jump(merge_block, &[]);
        builder.seal_block(drop_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);
        Ok(())
    }

    /// Decrements the RC of the existing element at `elem_addr` when the element
    /// type is a managed heap object (String, List, Array, Set, Map, user-defined
    /// class, Tuple, or Option).
    ///
    /// Called by `translate_collection_index_write` before the new value is stored
    /// so that overwriting an existing slot does not leak the old value.
    ///
    /// Routing: built-in collections / String use their per-shape runtime decref
    /// helper (the fast path); user classes call `miri.TypeName.$decref`; Tuple,
    /// Option and function values (the only `ElementShape::Other` variants that
    /// MIR releases, [`releases_slot`]) inline through
    /// `emit_decref_value`, which dispatches to `emit_type_drop` and
    /// recursively releases nested managed fields. Primitive `Other` shapes are
    /// the explicit no-op branch.
    pub(crate) fn emit_managed_elem_decref(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        elem_addr: Value,
        elem_type_kind: &TypeKind,
        ptr_type: cl_types::Type,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let shape = Self::classify_element_shape(elem_type_kind);
        let builtin_decref = match shape {
            ElementShape::String => Some(rt::STRING_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::List) => Some(rt::LIST_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Array) => Some(rt::ARRAY_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Set) => Some(rt::SET_DECREF_ELEMENT),
            ElementShape::Builtin(BuiltinCollectionKind::Map) => Some(rt::MAP_DECREF_ELEMENT),
            ElementShape::UserClass(_) | ElementShape::Other => None,
        };
        if let Some(name) = builtin_decref {
            let old_val = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), elem_addr, 0);
            Self::call_cached_func(
                builder,
                ctx.module,
                &mut ctx.cached_funcs,
                CallSite {
                    name,
                    param_types: &[ptr_type],
                    return_types: &[],
                    args: &[old_val],
                },
            )?;
            return Ok(());
        }
        if let ElementShape::UserClass(class_name) = shape {
            // Route a recorded generic-class instantiation (`Box<String>`) to its
            // per-instantiation `miri.Box$String.$decref` wrapper so the concrete
            // managed field is released; other classes use the bare name.
            // A trait's one `$decref` dispatches on the element's runtime
            // class, whatever arguments the trait is written at.
            let is_trait = matches!(
                type_ctx.facts.definitions().get(class_name),
                Some(TypeDefinition::Trait(_))
            );
            let per_instantiation = if is_trait {
                None
            } else {
                Self::drop_thunk(class_name, Self::custom_type_args(elem_type_kind), type_ctx)?
                    .per_instantiation()
            };
            let decref_name = Symbol::type_thunk(
                ThunkKind::Decref,
                class_name,
                per_instantiation.iter().flatten(),
            )
            .link_name();
            let old_val = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), elem_addr, 0);
            let func_id = match ctx.thunk_funcs.get(&decref_name) {
                Some(func_id) => *func_id,
                None => {
                    let sig = Signature {
                        params: vec![AbiParam::new(ptr_type)],
                        returns: vec![],
                        call_conv: builder.func.signature.call_conv,
                    };
                    let func_id = ctx
                        .module
                        .declare_function(&decref_name, Linkage::Import, &sig)
                        .map_err(|e| {
                            CodegenError::declare_function(decref_name.clone(), e.to_string())
                        })?;
                    ctx.thunk_funcs.insert(decref_name, func_id);
                    func_id
                }
            };
            let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
            builder.ins().call(local_func, &[old_val]);
            return Ok(());
        }
        // `Other` shapes the classifier counts (a boxed tuple or optional, a
        // function value) are heap-allocated with an RC header. Route through the inline
        // decref-and-drop emitter so nested managed payloads are released.
        if releases_slot(elem_type_kind, type_ctx.layout_scope()) {
            let old_val = builder
                .ins()
                .load(ptr_type, MemFlagsData::new(), elem_addr, 0);
            Self::emit_decref_value(builder, ctx, elem_type_kind, old_val, type_ctx)?;
        }
        Ok(())
    }

    /// Emits a call to the type-specific drop thunk `thunk(ptr)`.
    ///
    /// This is the sole call site for dropping a Custom type once RC reaches zero.
    /// The thunk function is declared as `Import` here and must be defined elsewhere
    /// (via `generate_drop_function`) before the final link.
    fn call_drop_thunk(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        thunk: &Symbol,
        ptr: Value,
        ptr_type: cranelift_codegen::ir::Type,
    ) -> Result<(), CodegenError> {
        let thunk_name = thunk.link_name();
        let mut sig = Signature::new(builder.func.signature.call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = ctx
            .module
            .declare_function(&thunk_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(thunk_name.clone(), e.to_string()))?;
        let local_func = ctx.module.declare_func_in_func(func_id, builder.func);
        builder.ins().call(local_func, &[ptr]);
        Ok(())
    }

    /// Emits an inline DecRef sequence for a managed value.
    ///
    /// Checks the RC header, decrements it, and if zero calls emit_type_drop
    /// recursively. This is the same logic as `StatementKind::DecRef` but for
    /// an arbitrary `Value` (not tied to a MIR local).
    pub(crate) fn emit_decref_value(
        builder: &mut FunctionBuilder,
        ctx: &mut ModuleCtx,
        kind: &TypeKind,
        ptr: Value,
        type_ctx: &TypeCtx,
    ) -> Result<(), CodegenError> {
        let ptr_type = type_ctx.ptr_type;
        let ptr_size = ptr_type.bytes() as i64;

        // Guard: skip if pointer is null
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        builder.switch_to_block(rc_block);
        Self::emit_release_check(builder, ctx, ptr)?;

        let header_ptr = builder.ins().iadd_imm_s(ptr, -ptr_size);
        let rc = builder.ins().load(
            ptr_type,
            cranelift_codegen::ir::MemFlagsData::new(),
            header_ptr,
            0,
        );

        // Skip immortal objects (RC < 0)
        let is_immortal = builder.ins().icmp_imm_s(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        builder.switch_to_block(dec_block);
        let new_rc = builder.ins().iadd_imm_s(rc, -1);
        builder.ins().store(
            cranelift_codegen::ir::MemFlagsData::new(),
            new_rc,
            header_ptr,
            0,
        );

        let zero = builder.ins().iconst(ptr_type, 0);
        let is_zero =
            builder
                .ins()
                .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, new_rc, zero);

        let free_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, free_block, &[], merge_block, &[]);

        builder.switch_to_block(free_block);
        Self::emit_rc_stats_hook(builder, ctx, rt::RC_STATS_FREE)?;
        Self::emit_type_drop(builder, ctx, kind, ptr, header_ptr, type_ctx)?;
        builder.ins().jump(merge_block, &[]);

        builder.seal_block(rc_block);
        builder.seal_block(dec_block);
        builder.seal_block(free_block);
        builder.switch_to_block(merge_block);
        builder.seal_block(merge_block);

        Ok(())
    }

    /// Generates the `miri.{type_name}.$drop(ptr)` function in the given module.
    ///
    /// The generated function implements the three-step destructor pipeline:
    /// 1. User-defined drop hook — invoked when the type defines `fn drop(self)`.
    /// 2. Recursively DecRef all managed fields.
    /// 3. Free the RC allocation via `libc::free`.
    ///
    /// This function is called once per managed concrete type during codegen,
    /// before any user functions are compiled, so the thunk symbols are available
    /// when user code later references them via Import declarations.
    /// `type_args = None` generates the bare `miri.TypeName.$drop`; `Some(args)`
    /// generates a per-instantiation thunk (`miri.Box$String.$drop`) whose body
    /// resolves each bare-generic field against `args`. This function emits only
    /// the bare thunk's matching `miri.TypeName.$decref` wrapper; the caller emits
    /// the per-instantiation `miri.Box$String.$decref` wrapper separately (via
    /// `generate_decref_function`) so a `List<Box<String>>` element's runtime
    /// decref helper routes through the per-instantiation drop.
    pub(crate) fn generate_drop_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_args: Option<&[Type]>,
        facts: &TypeFacts,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let func_name =
            Symbol::type_thunk(ThunkKind::Drop, type_name, type_args.unwrap_or_default())
                .link_name();
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&func_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(func_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_drop_body(
            module,
            ctx,
            &mut builder_ctx,
            type_name,
            type_args,
            facts,
            ptr_type,
            call_conv,
        )?;

        crate::codegen::cranelift::define_function(module, func_id, ctx)
            .map_err(|e| CodegenError::define_function(func_name, e.to_string()))?;
        ctx.clear();

        if type_args.is_some() {
            // The per-instantiation `miri.Box$String.$decref` wrapper is emitted by
            // the caller after this thunk is defined, keyed by the mangled name.
            return Ok(());
        }
        // Generate miri.TypeName.$decref: the RC-decrement wrapper used as
        // elem_drop_fn for collections holding custom-type elements.
        Self::generate_decref_function(module, ctx, isa, type_name, &[], facts.release(type_name))
    }

    /// Emit the body of `miri.TypeName.$drop(ptr)` from its plan:
    ///   1. run the `fn drop(self)` hook the plan names,
    ///   2. release every counted member the plan names,
    ///   3. free the RC allocation.
    #[allow(clippy::too_many_arguments)]
    fn emit_drop_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        type_name: &str,
        type_args: Option<&[Type]>,
        facts: &TypeFacts,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let ptr_size = ptr_type.bytes() as i64;
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let plan = facts.drop_plan(type_name, type_args).ok_or_else(|| {
            CodegenError::Internal(format!(
                "a drop thunk is emitted for {}, which has no drop plan",
                Symbol::type_thunk(ThunkKind::Drop, type_name, type_args.unwrap_or_default())
                    .written()
            ))
        })?;
        let open_params = facts.thunk_open_parameters(type_name, type_args);
        let no_bound_params = HashSet::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            facts,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
            open_params: &open_params,
            bound_params: &no_bound_params,
            values: &ValueLocals::none(),
            stable_headers: &crate::codegen::cranelift::stable_headers::StableHeaders::none(),
        };

        if let Some(hook) = &plan.hook {
            // The instance's count is already zero. The hook borrows `self`,
            // but a body that reads it as a whole value (`match self`) retains
            // and releases it, and that release would drop it again from
            // inside its own hook. Marked immortal, both leave it alone.
            let header_ptr = builder.ins().iadd_imm_s(ptr, -i64::from(ptr_type.bytes()));
            let immortal = builder.ins().iconst(ptr_type, -1);
            builder
                .ins()
                .store(MemFlagsData::new(), immortal, header_ptr, 0);
            Self::call_user_drop_hook(
                &mut builder,
                &mut module_ctx,
                &hook.link_name(),
                ptr,
                ptr_type,
                call_conv,
            )?;
        }

        Self::emit_planned_members(
            &mut builder,
            &mut module_ctx,
            type_name,
            type_args,
            &plan.members,
            ptr,
            &type_ctx,
        )?;

        let header_ptr = builder.ins().iadd_imm_s(ptr, -ptr_size);
        Self::call_rt_block_free(&mut builder, &mut module_ctx, header_ptr)?;

        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
        Ok(())
    }

    /// Emit a call to the user-defined drop hook `hook_name(self, allocator)`
    /// declared by `fn drop(self)`. ABI mirrors a method body lowered with its
    /// receiver and no explicit parameters: `(self: ptr, allocator: ptr) -> void`,
    /// with a null allocator placeholder.
    fn call_user_drop_hook(
        builder: &mut FunctionBuilder,
        module_ctx: &mut ModuleCtx,
        hook_name: &str,
        self_ptr: Value,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let receiver_and_allocator = [Passing::Word(ptr_type), Passing::Word(ptr_type)];
        let user_sig = miri_signature(
            call_conv,
            ptr_type,
            &receiver_and_allocator,
            Returning::Nothing,
        );
        let user_drop_id = module_ctx
            .module
            .declare_function(hook_name, Linkage::Import, &user_sig)
            .map_err(|e| CodegenError::declare_function(hook_name.to_string(), e.to_string()))?;
        let local_user_drop = module_ctx
            .module
            .declare_func_in_func(user_drop_id, builder.func);
        let zero = builder.ins().iconst(ptr_type, 0);
        builder.ins().call(local_user_drop, &[self_ptr, zero]);
        Ok(())
    }

    /// Generates the decref thunk of `type_name` at `type_args` — empty for the
    /// shared one — in the given module.
    ///
    /// Emits the RC-decrement pattern:
    ///   1. Guard: skip if ptr is null.
    ///   2. Guard: skip if RC < 0 (immortal).
    ///   3. Decrement RC.
    ///   4. If RC reaches zero, call the drop thunk of the same instantiation.
    ///
    /// Used as `elem_drop_fn` for List/Set/Map holding custom-type elements so
    /// that mutation operations (clear, remove_at, remove) properly DecRef
    /// each removed instance.
    pub(crate) fn generate_decref_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        type_args: &[Type],
        release: Release,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let decref_name = Symbol::type_thunk(ThunkKind::Decref, type_name, type_args).link_name();
        let drop_thunk = match release {
            Release::Trait => None,
            Release::Exact | Release::ByRuntimeClass => {
                Some(Symbol::type_thunk(ThunkKind::Drop, type_name, type_args))
            }
        };
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&decref_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig.clone(),
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_decref_body(
            module,
            ctx,
            &mut builder_ctx,
            drop_thunk.as_ref(),
            release,
            ptr_type,
            &sig,
        )?;

        crate::codegen::cranelift::define_function(module, func_id, ctx)
            .map_err(|e| CodegenError::define_function(decref_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Generates the decref thunk of a structural type — a tuple, an option or
    /// a function value, which carries managed payload but has no declaration
    /// whose name could be mangled into a symbol. `encoding` comes from
    /// [`crate::codegen::cranelift::structural_elements::structural_thunk_symbol`],
    /// which encodes the type's structure so distinct types get distinct thunks.
    ///
    /// The body is the same inline decref-and-drop sequence a direct release
    /// emits, so the structural payload is released exactly as it would be at a
    /// binding's scope exit. Used as a map's `key_drop_fn`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn generate_structural_decref_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        element: &StructuralElement,
        facts: &TypeFacts,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let decref_name = Symbol::structural_thunk(ThunkKind::Decref, &element.symbol).link_name();
        let mut sig = Signature::new(isa.default_call_conv());
        sig.params.push(AbiParam::new(ptr_type));
        let func_id = module
            .declare_function(&decref_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(decref_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );
        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_structural_decref_body(module, ctx, &mut builder_ctx, element, facts, ptr_type)?;

        crate::codegen::cranelift::define_function(module, func_id, ctx)
            .map_err(|e| CodegenError::define_function(decref_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of a structural decref thunk: release `ptr` at `kind`,
    /// which drops and frees it when its count reaches zero, then return.
    fn emit_structural_decref_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        element: &StructuralElement,
        facts: &TypeFacts,
        ptr_type: cl_types::Type,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);
        let empty_captures = HashMap::new();
        let empty_out_ptr_vars = HashMap::new();
        let type_ctx = TypeCtx {
            local_types: &[],
            facts,
            ptr_type,
            closure_capture_ast_types: &empty_captures,
            out_param_ptr_vars: &empty_out_ptr_vars,
            open_params: &element.open_params,
            bound_params: &element.bound_params,
            values: &ValueLocals::none(),
            stable_headers: &crate::codegen::cranelift::stable_headers::StableHeaders::none(),
        };

        Self::emit_decref_value(&mut builder, &mut module_ctx, &element.kind, ptr, &type_ctx)?;
        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
        Ok(())
    }

    /// Emit the body of a decref thunk: null guard → heap-guard release check →
    /// immortal guard → decrement RC → when RC hits zero call `drop_thunk(ptr)`
    /// → return.
    #[allow(clippy::too_many_arguments)]
    fn emit_decref_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        drop_thunk: Option<&Symbol>,
        release: Release,
        ptr_type: cl_types::Type,
        sig: &Signature,
    ) -> Result<(), CodegenError> {
        let ptr_size = ptr_type.bytes() as i64;
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        let mut string_literals = BTreeMap::new();
        let empty_kernel_registry = HashMap::new();
        let mut module_ctx = empty_module_ctx(module, &mut string_literals, &empty_kernel_registry);

        // Null guard.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let rc_block = builder.create_block();
        let merge_block = builder.create_block();
        builder.ins().brif(is_null, merge_block, &[], rc_block, &[]);

        // Report the release to the heap guard, then load RC + check immortal
        // flag (high bit set).
        builder.switch_to_block(rc_block);
        builder.seal_block(rc_block);
        Self::emit_release_check(&mut builder, &mut module_ctx, ptr)?;
        let header_ptr = builder.ins().iadd_imm_s(ptr, -ptr_size);
        let rc = builder
            .ins()
            .load(ptr_type, MemFlagsData::new(), header_ptr, 0);
        let is_immortal = builder.ins().icmp_imm_s(
            cranelift_codegen::ir::condcodes::IntCC::SignedLessThan,
            rc,
            0,
        );
        let dec_block = builder.create_block();
        builder
            .ins()
            .brif(is_immortal, merge_block, &[], dec_block, &[]);

        // Decrement RC; branch to `__drop` thunk when it reaches zero.
        builder.switch_to_block(dec_block);
        builder.seal_block(dec_block);
        let new_rc = builder.ins().iadd_imm_s(rc, -1);
        builder
            .ins()
            .store(MemFlagsData::new(), new_rc, header_ptr, 0);
        let zero = builder.ins().iconst(ptr_type, 0);
        let is_zero =
            builder
                .ins()
                .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, new_rc, zero);
        let free_block = builder.create_block();
        builder
            .ins()
            .brif(is_zero, free_block, &[], merge_block, &[]);

        builder.switch_to_block(free_block);
        builder.seal_block(free_block);
        Self::emit_rc_stats_hook(&mut builder, &mut module_ctx, rt::RC_STATS_FREE)?;
        let static_release =
            |builder: &mut FunctionBuilder, module_ctx: &mut ModuleCtx| match drop_thunk {
                Some(thunk) => Self::call_type_drop(builder, module_ctx.module, thunk, sig, ptr),
                None => Self::call_rt_block_free(builder, module_ctx, header_ptr),
            };
        match release {
            Release::Exact => static_release(&mut builder, &mut module_ctx)?,
            Release::ByRuntimeClass | Release::Trait => Self::emit_release_by_runtime_class(
                &mut builder,
                &mut module_ctx,
                ptr,
                ptr_type,
                static_release,
            )?,
        }
        builder.ins().jump(merge_block, &[]);

        builder.switch_to_block(merge_block);
        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
        Ok(())
    }

    /// Emit the call `drop_thunk(ptr)`, which runs the type's drop hook,
    /// releases its managed fields and frees it. `sig` is the drop thunk's
    /// `(ptr) -> void` signature.
    fn call_type_drop(
        builder: &mut FunctionBuilder,
        module: &mut ObjectModule,
        drop_thunk: &Symbol,
        sig: &Signature,
        ptr: Value,
    ) -> Result<(), CodegenError> {
        let drop_name = drop_thunk.link_name();
        let drop_func_id = module
            .declare_function(&drop_name, Linkage::Import, sig)
            .map_err(|e| CodegenError::declare_function(drop_name.clone(), e.to_string()))?;
        let local_drop = module.declare_func_in_func(drop_func_id, builder.func);
        builder.ins().call(local_drop, &[ptr]);
        Ok(())
    }

    /// Generates `miri.{type_name}.$clone(ptr) -> ptr` for each concrete class that
    /// implements `Cloneable`.
    ///
    /// This function is used as `elem_clone_fn` in Array/List/Set so that
    /// `miri_rt_XXX_clone` produces independent element copies instead of just
    /// IncRef-ing shared pointers.  It delegates to the user's compiled `clone()`
    /// method (`{TypeName}_clone`), which already encodes any deep-copy logic.
    ///
    /// Only generated for concrete (non-generic, non-abstract) classes whose
    /// `traits` list includes `"Cloneable"` (checked with inheritance walk).
    pub(crate) fn generate_clone_function(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        inst_args: Option<&[Type]>,
        facts: &TypeFacts,
    ) -> Result<(), CodegenError> {
        // Abstract classes may have an abstract clone() with no compiled body;
        // a thunk for them would reference an undefined symbol at link time.
        if !facts.has_clone_thunk(type_name) {
            return Ok(());
        }
        let clone_method = Self::clone_method_symbol(type_name, inst_args, facts);
        if facts.is_withheld(&clone_method) {
            return Ok(());
        }

        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();

        let clone_name =
            Symbol::type_thunk(ThunkKind::Clone, type_name, inst_args.unwrap_or_default())
                .link_name();

        // Signature: (ptr: *TypeName) -> *TypeName
        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns.push(AbiParam::new(ptr_type));

        let func_id = module
            .declare_function(&clone_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(clone_name.clone(), e.to_string()))?;

        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig.clone(),
        );

        let mut builder_ctx = FunctionBuilderContext::new();
        Self::emit_clone_body(
            module,
            ctx,
            &mut builder_ctx,
            clone_method.link_name(),
            ptr_type,
            call_conv,
        )?;

        crate::codegen::cranelift::define_function(module, func_id, ctx)
            .map_err(|e| CodegenError::define_function(clone_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }

    /// Emit the body of `miri.TypeName.$clone(ptr)`: null guard → call the
    /// `clone()` body `clone_method_name` names, resolved through the
    /// inheritance chain at the instantiation the thunk is for → return the
    /// result.
    #[allow(clippy::too_many_arguments)]
    fn emit_clone_body(
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        builder_ctx: &mut FunctionBuilderContext,
        clone_method_name: String,
        ptr_type: cl_types::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);
        let ptr = builder.block_params(entry_block)[0];

        // Null guard: return null if ptr is null.
        let null = builder.ins().iconst(ptr_type, 0);
        let is_null = builder
            .ins()
            .icmp(cranelift_codegen::ir::condcodes::IntCC::Equal, ptr, null);
        let null_ret_block = builder.create_block();
        let call_block = builder.create_block();
        builder
            .ins()
            .brif(is_null, null_ret_block, &[], call_block, &[]);

        builder.switch_to_block(null_ret_block);
        builder.seal_block(null_ret_block);
        builder.ins().return_(&[null]);

        builder.switch_to_block(call_block);
        builder.seal_block(call_block);

        let receiver_and_allocator = [Passing::Word(ptr_type), Passing::Word(ptr_type)];
        let user_clone_sig = miri_signature(
            call_conv,
            ptr_type,
            &receiver_and_allocator,
            Returning::Word(ptr_type),
        );

        let user_clone_id = module
            .declare_function(&clone_method_name, Linkage::Import, &user_clone_sig)
            .map_err(|e| CodegenError::declare_function(clone_method_name, e.to_string()))?;
        let local_fn = module.declare_func_in_func(user_clone_id, builder.func);
        let zero = builder.ins().iconst(ptr_type, 0);
        let inst = builder.ins().call(local_fn, &[ptr, zero]);
        let result = builder.inst_results(inst)[0];
        builder.ins().return_(&[result]);

        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
        Ok(())
    }

    /// Resolves the mangled name of the shared `clone()` body for `type_name`.
    ///
    /// The symbol is the one a static call to `clone` on a `type_name`
    /// receiver names, as [`TypeFacts::element_method_symbol`] answers it.
    pub fn resolve_clone_method_name(type_name: &str, facts: &TypeFacts) -> String {
        Self::clone_method_symbol(type_name, None, facts).link_name()
    }
}
