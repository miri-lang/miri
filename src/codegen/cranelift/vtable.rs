// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Vtable layout and resolution for abstract-class / trait dispatch.
//!
//! Generates one `miri.{Class}[${args}].$vtable` data symbol per class
//! instantiation a compiled body builds, for use by
//! `TerminatorKind::VirtualCall`. Which vtables exist, the slots each fills and
//! the symbol each slot names all come from the dispatch data the pipeline
//! settled in `mir::dispatch`.

use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::error::CodegenError;
use crate::mir::dispatch::{FilledSlot, VtableLayout};
use crate::mir::type_facts::TypeFacts;

use cranelift_module::Module;
use cranelift_object::ObjectModule;

impl<'a> FunctionTranslator<'a> {
    /// Define each of `vtables`, a symbol with the slots it fills, in order.
    ///
    /// Every vtable is an array of function pointers laid out by the one
    /// program-wide [`VtableLayout`] the lowered `VirtualCall`s index by. A
    /// slot the pipeline filled points to the body it names, or, where
    /// lowering withheld that body at this instance, to the runtime trap that
    /// reports it; every other slot is null, and no reached call reads it.
    ///
    /// Must be called AFTER all function bodies are compiled: a slot names a
    /// body the module already defines, and one it does not is reported.
    pub(crate) fn generate_vtables<'v>(
        module: &mut ObjectModule,
        ptr_type: cranelift_codegen::ir::Type,
        facts: &TypeFacts,
        vtables: impl IntoIterator<Item = (&'v str, &'v [FilledSlot])>,
    ) -> Result<(), CodegenError> {
        let slot_count = VtableLayout::of(facts.definitions()).slot_count();
        for (symbol, slots) in vtables {
            Self::emit_vtable(module, ptr_type, slot_count, symbol, slots, facts)?;
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
        facts: &TypeFacts,
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
            let func_id = if facts.is_withheld(&filled.symbol) {
                Self::method_not_checked_trap(module)?
            } else {
                Self::vtable_slot_func_id(module, symbol, &filled.method, &filled.symbol)?
            };
            let func_ref = module.declare_func_in_data(func_id, &mut desc);
            desc.write_function_addr((filled.slot * ptr_size) as u32, func_ref);
        }

        module
            .define_data(vtable_data_id, &desc)
            .map_err(|e| CodegenError::define_function(symbol.to_string(), e.to_string()))
    }

    /// The runtime function that reports a method reached at an instance its
    /// obligations were not checked at. It takes no arguments, so a slot of any
    /// signature may point to it: the caller's arguments are ignored.
    pub(crate) fn method_not_checked_trap(
        module: &mut ObjectModule,
    ) -> Result<cranelift_module::FuncId, CodegenError> {
        let name = crate::runtime_fns::rt::METHOD_NOT_CHECKED_PANIC;
        let signature = module.make_signature();
        module
            .declare_function(name, cranelift_module::Linkage::Import, &signature)
            .map_err(|e| CodegenError::declare_function(name.to_string(), e.to_string()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mir::dispatch::VtableFills;
    use cranelift_module::FuncOrDataId;
    use std::collections::{HashMap, HashSet};

    fn host_module() -> ObjectModule {
        let flags = cranelift_codegen::settings::Flags::new(cranelift_codegen::settings::builder());
        let Ok(isa_builder) = cranelift_native::builder() else {
            panic!("the host has a Cranelift backend");
        };
        let Ok(isa) = isa_builder.finish(flags) else {
            panic!("the host ISA builds");
        };
        let Ok(builder) = cranelift_object::ObjectBuilder::new(
            isa,
            "vtable_test",
            cranelift_module::default_libcall_names(),
        ) else {
            panic!("an object module builds");
        };
        ObjectModule::new(builder)
    }

    fn facts_withholding(withheld: &[&str]) -> TypeFacts {
        let Ok(facts) = TypeFacts::new(HashMap::new(), HashMap::new(), VtableFills::default(), [])
        else {
            panic!("empty facts settle");
        };
        facts.withholding(
            withheld
                .iter()
                .map(|name| name.to_string())
                .collect::<HashSet<_>>(),
        )
    }

    /// A slot whose method lowering withheld at the instance points to the
    /// runtime trap, not to a body the module never defines and not to null.
    #[test]
    fn a_withheld_slot_names_the_method_not_checked_trap() {
        let mut module = host_module();
        let slots = [FilledSlot {
            slot: 0,
            method: "bad".to_string(),
            symbol: "miri.A.bad".to_string(),
        }];
        let facts = facts_withholding(&["miri.A.bad"]);
        let emitted = FunctionTranslator::emit_vtable(
            &mut module,
            cranelift_codegen::ir::types::I64,
            1,
            "vt",
            &slots,
            &facts,
        );
        assert!(emitted.is_ok(), "{emitted:?}");
        assert!(
            matches!(
                module.get_name(crate::runtime_fns::rt::METHOD_NOT_CHECKED_PANIC),
                Some(FuncOrDataId::Func(_))
            ),
            "the withheld slot must be filled with the trap"
        );
    }

    /// A slot whose body was not withheld still has to name a compiled body.
    #[test]
    fn a_slot_naming_no_compiled_body_is_reported() {
        let mut module = host_module();
        let slots = [FilledSlot {
            slot: 0,
            method: "keep".to_string(),
            symbol: "miri.A.keep".to_string(),
        }];
        let facts = facts_withholding(&[]);
        let emitted = FunctionTranslator::emit_vtable(
            &mut module,
            cranelift_codegen::ir::types::I64,
            1,
            "vt",
            &slots,
            &facts,
        );
        assert!(emitted.is_err(), "a missing body must be reported");
    }
}
