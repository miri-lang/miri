// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Vtable layout and resolution for abstract-class / trait dispatch.
//!
//! Generates per-class `__vtable_{ClassName}` data symbols for use by
//! `TerminatorKind::VirtualCall`. Which classes get one, their slots and the
//! symbol each slot names come from `mir::lowering::dispatch_symbols`.

use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::error::CodegenError;
use crate::mir::lowering::dispatch_symbols;
use crate::type_checker::context::TypeDefinition;

use cranelift_codegen::isa::TargetIsa;
use cranelift_module::Module;
use cranelift_object::ObjectModule;
use std::collections::HashMap;
use std::sync::Arc;

impl<'a> FunctionTranslator<'a> {
    /// Generate `__vtable_ClassName` static data for each concrete class that
    /// participates in virtual dispatch (has an abstract class or a trait in
    /// its hierarchy).
    ///
    /// Every vtable is an array of function pointers laid out by the one
    /// program-wide [`dispatch_symbols::VtableLayout`] the lowered
    /// `VirtualCall`s index by. A slot the class gives a body points to the
    /// implementation resolved from its inheritance chain; every other slot
    /// is null, and no well-typed call reads it.
    ///
    /// A class whose traits require no method still gets its symbol: its
    /// constructor stores the vtable pointer all the same, by the one
    /// `class_needs_vtable` rule both sides read.
    ///
    /// Must be called AFTER all user function bodies are compiled, so the function
    /// symbols are registered in the module.
    pub(crate) fn generate_vtables(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let layout = dispatch_symbols::VtableLayout::of(type_definitions);
        let classes = dispatch_symbols::collect_classes_needing_vtable(type_definitions);
        for (class_name, _) in &classes {
            let vtable_methods =
                dispatch_symbols::collect_vtable_methods(class_name, type_definitions);
            Self::emit_vtable_for_class(
                module,
                isa,
                &layout,
                class_name,
                &vtable_methods,
                type_definitions,
            )?;
        }
        Ok(())
    }

    /// Declare-and-define one `__vtable_{class_name}` data symbol with a
    /// pointer slot for every name in `layout`, filling the slot of each of
    /// `vtable_methods` with the body `dispatch_symbols::resolve_vtable_method`
    /// resolves. Method symbols that are not yet declared in the module are
    /// imported with a placeholder signature.
    fn emit_vtable_for_class(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        layout: &dispatch_symbols::VtableLayout,
        class_name: &str,
        vtable_methods: &[&str],
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        use cranelift_module::Linkage;
        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes() as usize;
        let vtable_sym = format!("__vtable_{class_name}");
        let vtable_data_id = module
            .declare_data(&vtable_sym, Linkage::Export, false, false)
            .map_err(|e| CodegenError::declare_function(vtable_sym.clone(), e.to_string()))?;

        let mut desc = cranelift_module::DataDescription::new();
        desc.set_align(ptr_size as u64);
        desc.define(vec![0u8; layout.slot_count() * ptr_size].into_boxed_slice());

        for method_name in vtable_methods {
            let Some(func_name) =
                dispatch_symbols::resolve_vtable_method(class_name, method_name, type_definitions)
            else {
                continue;
            };
            let slot = layout.slot(method_name).ok_or_else(|| {
                CodegenError::Internal(format!(
                    "vtable of `{class_name}`: method `{method_name}` has no slot in the layout"
                ))
            })?;
            let func_id =
                Self::vtable_slot_func_id(module, &func_name, ptr_type, isa.default_call_conv())?;
            let func_ref = module.declare_func_in_data(func_id, &mut desc);
            desc.write_function_addr((slot * ptr_size) as u32, func_ref);
        }

        module
            .define_data(vtable_data_id, &desc)
            .map_err(|e| CodegenError::define_function(vtable_sym, e.to_string()))
    }

    /// Look up `func_name` in the module; declare it as `Linkage::Import` with
    /// a placeholder `(ptr) -> ()` signature when missing. Covers abstract-
    /// base concrete methods only invoked through vtable dispatch.
    fn vtable_slot_func_id(
        module: &mut ObjectModule,
        func_name: &str,
        ptr_type: cranelift_codegen::ir::Type,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<cranelift_module::FuncId, CodegenError> {
        use cranelift_module::{FuncOrDataId, Linkage};
        if let Some(FuncOrDataId::Func(id)) = module.get_name(func_name) {
            return Ok(id);
        }
        let mut sig = cranelift_codegen::ir::Signature::new(call_conv);
        sig.params
            .push(cranelift_codegen::ir::AbiParam::new(ptr_type));
        module
            .declare_function(func_name, Linkage::Import, &sig)
            .map_err(|e| CodegenError::declare_function(func_name.to_string(), e.to_string()))
    }
}
