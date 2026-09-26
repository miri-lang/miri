// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Vtable layout and resolution for abstract-class / trait dispatch.
//!
//! Generates one `miri.{Class}[${args}].$vtable` data symbol per class
//! instantiation a compiled body builds, for use by
//! `TerminatorKind::VirtualCall`. Which vtables exist comes from
//! `mir::lowering::dispatch_symbols`; the slots each fills and the symbol each
//! slot names, from the `mir::lowering::vtable_demand` the pipeline settled.

use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::error::CodegenError;
use crate::mir::lowering::dispatch_symbols::VtableLayout;
use crate::mir::lowering::vtable_demand::FilledSlot;
use crate::type_checker::context::TypeDefinition;

use cranelift_module::Module;
use cranelift_object::ObjectModule;
use std::collections::HashMap;

impl<'a> FunctionTranslator<'a> {
    /// Define each of `vtables`, a symbol with the slots it fills, in order.
    ///
    /// Every vtable is an array of function pointers laid out by the one
    /// program-wide [`VtableLayout`] the lowered `VirtualCall`s index by. A
    /// slot the pipeline filled points to the body it names; every other slot
    /// is null, and no reached call reads it.
    ///
    /// Must be called AFTER all function bodies are compiled: a slot names a
    /// body the module already defines, and one it does not is reported.
    pub(crate) fn generate_vtables<'v>(
        module: &mut ObjectModule,
        ptr_type: cranelift_codegen::ir::Type,
        type_definitions: &HashMap<String, TypeDefinition>,
        vtables: impl IntoIterator<Item = (&'v str, &'v [FilledSlot])>,
    ) -> Result<(), CodegenError> {
        let slot_count = VtableLayout::of(type_definitions).slot_count();
        for (symbol, slots) in vtables {
            Self::emit_vtable(module, ptr_type, slot_count, symbol, slots)?;
        }
        Ok(())
    }

    /// Declare-and-define the vtable data symbol `symbol` with `slot_count`
    /// pointer slots, each of `slots` holding the address of the function it
    /// names for its method and every other one null.
    fn emit_vtable(
        module: &mut ObjectModule,
        ptr_type: cranelift_codegen::ir::Type,
        slot_count: usize,
        symbol: &str,
        slots: &[FilledSlot],
    ) -> Result<(), CodegenError> {
        use cranelift_module::Linkage;
        let ptr_size = ptr_type.bytes() as usize;
        let vtable_data_id = module
            .declare_data(symbol, Linkage::Export, false, false)
            .map_err(|e| CodegenError::declare_function(symbol.to_string(), e.to_string()))?;

        let mut desc = cranelift_module::DataDescription::new();
        desc.set_align(ptr_size as u64);
        desc.define(vec![0u8; slot_count * ptr_size].into_boxed_slice());

        for filled in slots {
            let func_id =
                Self::vtable_slot_func_id(module, symbol, &filled.method, &filled.symbol)?;
            let func_ref = module.declare_func_in_data(func_id, &mut desc);
            desc.write_function_addr((filled.slot * ptr_size) as u32, func_ref);
        }

        module
            .define_data(vtable_data_id, &desc)
            .map_err(|e| CodegenError::define_function(symbol.to_string(), e.to_string()))
    }

    /// The function `func_name` names in the module. A slot target the
    /// pipeline did not compile is a disagreement between the bodies it lowered
    /// and the vtable naming them, reported with the vtable and method rather
    /// than left to fail at link time.
    fn vtable_slot_func_id(
        module: &ObjectModule,
        vtable: &str,
        method_name: &str,
        func_name: &str,
    ) -> Result<cranelift_module::FuncId, CodegenError> {
        use cranelift_module::FuncOrDataId;
        match module.get_name(func_name) {
            Some(FuncOrDataId::Func(id)) => Ok(id),
            Some(FuncOrDataId::Data(_)) | None => Err(CodegenError::Internal(format!(
                "vtable `{vtable}`: the slot for `{method_name}` names `{func_name}`, which no compiled body defines"
            ))),
        }
    }
}
