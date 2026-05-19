// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Vtable layout and resolution for abstract-class / trait dispatch.
//!
//! Generates per-class `__vtable_{ClassName}` data symbols and resolves
//! virtual method names to vtable slot indices for use by
//! `TerminatorKind::VirtualCall`.

use crate::codegen::cranelift::translator::FunctionTranslator;
use crate::error::CodegenError;
use crate::type_checker::context::{ClassDefinition, TypeDefinition};

use cranelift_codegen::isa::TargetIsa;
use cranelift_module::Module;
use cranelift_object::ObjectModule;
use std::collections::HashMap;
use std::sync::Arc;

impl<'a> FunctionTranslator<'a> {
    /// Generate `__vtable_ClassName` static data for each concrete class that
    /// participates in virtual dispatch (has an abstract class in its hierarchy).
    ///
    /// The vtable is an array of function pointers in alphabetical order of the
    /// abstract interface class's non-constructor methods. Each slot points to the
    /// concrete implementation resolved from the class's inheritance chain.
    ///
    /// Must be called AFTER all user function bodies are compiled, so the function
    /// symbols are registered in the module.
    pub(crate) fn generate_vtables(
        module: &mut ObjectModule,
        isa: &Arc<dyn TargetIsa>,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        let ptr_type = isa.pointer_type();
        let ptr_size = ptr_type.bytes();
        let call_conv = isa.default_call_conv();

        let classes = Self::collect_classes_needing_vtable(type_definitions);
        for (class_name, _) in &classes {
            let vtable_methods = Self::collect_vtable_methods(class_name, type_definitions);
            if vtable_methods.is_empty() {
                continue;
            }
            Self::emit_vtable_for_class(
                module,
                class_name,
                &vtable_methods,
                type_definitions,
                ptr_type,
                ptr_size,
                call_conv,
            )?;
        }
        Ok(())
    }

    /// Return the concrete (non-abstract) classes that participate in virtual
    /// dispatch, sorted by class name for deterministic codegen output.
    /// Generic classes are included — their methods are compiled once with
    /// `T` treated as a pointer-sized opaque slot.
    fn collect_classes_needing_vtable(
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Vec<(&str, &ClassDefinition)> {
        use crate::type_checker::context::class_needs_vtable;
        let mut classes: Vec<(&str, &ClassDefinition)> = type_definitions
            .iter()
            .filter_map(|(name, def)| {
                let TypeDefinition::Class(cd) = def else {
                    return None;
                };
                if !cd.is_abstract && class_needs_vtable(name, type_definitions) {
                    Some((name.as_str(), cd))
                } else {
                    None
                }
            })
            .collect();
        classes.sort_unstable_by_key(|(name, _)| *name);
        classes
    }

    /// Collect the vtable's method-name slots for `class_name`. Walks the
    /// abstract-ancestor chain (root → leaf order so base methods come first)
    /// and merges in trait-required methods from every implemented trait.
    /// Results are sorted alphabetically for deterministic slot indices.
    fn collect_vtable_methods<'td>(
        class_name: &str,
        type_definitions: &'td HashMap<String, TypeDefinition>,
    ) -> Vec<&'td str> {
        use crate::type_checker::context::collect_trait_vtable_methods;

        // 1. Walk inheritance chain, recording every abstract ancestor.
        let mut abstract_chain: Vec<&str> = Vec::new();
        let mut current: &str = class_name;
        loop {
            match type_definitions.get(current) {
                Some(TypeDefinition::Class(cd)) if cd.is_abstract => {
                    abstract_chain.push(current);
                    match &cd.base_class {
                        Some(base) => current = base,
                        None => break,
                    }
                }
                Some(TypeDefinition::Class(cd)) => match &cd.base_class {
                    Some(base) => current = base,
                    None => break,
                },
                None
                | Some(TypeDefinition::Struct(_))
                | Some(TypeDefinition::Enum(_))
                | Some(TypeDefinition::Generic(_))
                | Some(TypeDefinition::Alias(_))
                | Some(TypeDefinition::Trait(_)) => break,
            }
        }

        // 2. Collect methods from abstract ancestors (base-first ordering).
        let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
        let mut vtable_methods: Vec<&str> = Vec::new();
        for ancestor in abstract_chain.iter().rev() {
            if let Some(TypeDefinition::Class(cd)) = type_definitions.get(*ancestor) {
                for (method_name, method_info) in &cd.methods {
                    if !method_info.is_constructor && !seen.contains(method_name.as_str()) {
                        seen.insert(method_name.as_str());
                        vtable_methods.push(method_name.as_str());
                    }
                }
            }
        }

        // 3. Merge in trait-required methods from the class and every ancestor.
        let mut walk: &str = class_name;
        while let Some(TypeDefinition::Class(cd)) = type_definitions.get(walk) {
            for trait_name in &cd.traits {
                for m in collect_trait_vtable_methods(type_definitions, trait_name) {
                    if !seen.contains(m) {
                        seen.insert(m);
                        vtable_methods.push(m);
                    }
                }
            }
            match &cd.base_class {
                Some(b) => walk = b,
                None => break,
            }
        }

        vtable_methods.sort();
        vtable_methods
    }

    /// Declare-and-define one `__vtable_{class_name}` data symbol with one
    /// pointer slot per method (resolved through the class's inheritance chain
    /// via `resolve_vtable_method`). Method symbols that are not yet declared
    /// in the module are imported with a placeholder signature.
    #[allow(clippy::too_many_arguments)]
    fn emit_vtable_for_class(
        module: &mut ObjectModule,
        class_name: &str,
        vtable_methods: &[&str],
        type_definitions: &HashMap<String, TypeDefinition>,
        ptr_type: cranelift_codegen::ir::Type,
        ptr_size: u32,
        call_conv: cranelift_codegen::isa::CallConv,
    ) -> Result<(), CodegenError> {
        use cranelift_module::Linkage;
        let vtable_size = (vtable_methods.len() * ptr_size as usize) as u32;

        let mut vtable_sym = String::with_capacity(9 + class_name.len());
        vtable_sym.push_str("__vtable_");
        vtable_sym.push_str(class_name);
        let vtable_data_id = module
            .declare_data(&vtable_sym, Linkage::Export, false, false)
            .map_err(|e| CodegenError::declare_function(vtable_sym.clone(), e.to_string()))?;

        let mut desc = cranelift_module::DataDescription::new();
        desc.set_align(ptr_size as u64);
        desc.define(vec![0u8; vtable_size as usize].into_boxed_slice());

        for (slot_idx, method_name) in vtable_methods.iter().enumerate() {
            let Some(func_name) =
                Self::resolve_vtable_method(class_name, method_name, type_definitions)
            else {
                continue;
            };
            let func_id = Self::vtable_slot_func_id(module, &func_name, ptr_type, call_conv)?;
            let func_ref = module.declare_func_in_data(func_id, &mut desc);
            let slot_offset = (slot_idx * ptr_size as usize) as u32;
            desc.write_function_addr(slot_offset, func_ref);
        }

        module
            .define_data(vtable_data_id, &desc)
            .map_err(|e| CodegenError::define_function(vtable_sym.clone(), e.to_string()))
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

    /// Resolve which function implements `method_name` for `class_name` via inheritance.
    /// Returns `"ClassName_methodName"` for the first class in the chain that defines it,
    /// or `"TraitName_methodName"` if the method is a default trait implementation.
    pub(crate) fn resolve_vtable_method(
        class_name: &str,
        method_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        let mut current: &str = class_name;
        let mut all_traits: Vec<&str> = Vec::new();
        while let Some(TypeDefinition::Class(cd)) = type_definitions.get(current) {
            if let Some(method) = cd.methods.get(method_name) {
                // Only use this implementation if it has a body (not abstract).
                if !method.is_abstract {
                    let mut mangled = String::with_capacity(current.len() + 1 + method_name.len());
                    mangled.push_str(current);
                    mangled.push('_');
                    mangled.push_str(method_name);
                    return Some(mangled);
                }
            }
            all_traits.extend(cd.traits.iter().map(|s| s.as_str()));
            match &cd.base_class {
                Some(base) => current = base,
                None => break,
            }
        }
        // Fall back to a default trait method implementation.
        let mut visited = std::collections::HashSet::new();
        let mut trait_stack = all_traits;
        while let Some(t_name) = trait_stack.pop() {
            if !visited.insert(t_name) {
                continue;
            }
            if let Some(TypeDefinition::Trait(td)) = type_definitions.get(t_name) {
                if let Some(method) = td.methods.get(method_name) {
                    if !method.is_abstract {
                        let mut mangled =
                            String::with_capacity(t_name.len() + 1 + method_name.len());
                        mangled.push_str(t_name);
                        mangled.push('_');
                        mangled.push_str(method_name);
                        return Some(mangled);
                    }
                }
                trait_stack.extend(td.parent_traits.iter().map(|s| s.as_str()));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::{Type, TypeKind};
    use crate::ast::MemberVisibility;
    use crate::error::syntax::Span;
    use crate::type_checker::context::{ClassDefinition, MethodInfo, TraitDefinition};
    use std::collections::BTreeMap;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn void_type() -> Type {
        Type::new(TypeKind::Void, span())
    }

    fn method(is_abstract: bool, is_constructor: bool) -> MethodInfo {
        MethodInfo {
            params: Vec::new(),
            is_out_flags: Vec::new(),
            return_type: void_type(),
            visibility: MemberVisibility::Public,
            is_constructor,
            is_abstract,
        }
    }

    fn class(
        name: &str,
        base: Option<&str>,
        traits: &[&str],
        methods: &[(&str, MethodInfo)],
        is_abstract: bool,
    ) -> ClassDefinition {
        let mut method_map: BTreeMap<String, MethodInfo> = BTreeMap::new();
        for (n, m) in methods {
            method_map.insert(n.to_string(), m.clone());
        }
        ClassDefinition {
            name: name.to_string(),
            generics: None,
            base_class: base.map(String::from),
            base_class_args: None,
            traits: traits.iter().map(|s| s.to_string()).collect(),
            fields: Vec::new(),
            methods: method_map,
            module: String::new(),
            is_abstract,
            has_drop: false,
        }
    }

    fn trait_def(name: &str, parents: &[&str], methods: &[(&str, MethodInfo)]) -> TraitDefinition {
        let mut method_map: BTreeMap<String, MethodInfo> = BTreeMap::new();
        for (n, m) in methods {
            method_map.insert(n.to_string(), m.clone());
        }
        TraitDefinition {
            name: name.to_string(),
            generics: None,
            parent_traits: parents.iter().map(|s| s.to_string()).collect(),
            parent_trait_args: BTreeMap::new(),
            methods: method_map,
            module: String::new(),
        }
    }

    fn make_defs<I: IntoIterator<Item = (String, TypeDefinition)>>(
        entries: I,
    ) -> HashMap<String, TypeDefinition> {
        entries.into_iter().collect()
    }

    #[test]
    fn resolve_vtable_method_picks_concrete_override() {
        let base = class("Base", None, &[], &[("greet", method(true, false))], true);
        let derived = class(
            "Derived",
            Some("Base"),
            &[],
            &[("greet", method(false, false))],
            false,
        );
        let defs = make_defs([
            ("Base".to_string(), TypeDefinition::Class(base)),
            ("Derived".to_string(), TypeDefinition::Class(derived)),
        ]);
        assert_eq!(
            FunctionTranslator::resolve_vtable_method("Derived", "greet", &defs),
            Some("Derived_greet".to_string()),
        );
    }

    #[test]
    fn resolve_vtable_method_walks_to_base_when_derived_is_abstract() {
        let base = class("Base", None, &[], &[("greet", method(false, false))], false);
        let mid = class(
            "Mid",
            Some("Base"),
            &[],
            &[("greet", method(true, false))],
            true,
        );
        let defs = make_defs([
            ("Base".to_string(), TypeDefinition::Class(base)),
            ("Mid".to_string(), TypeDefinition::Class(mid)),
        ]);
        // Mid declares greet abstract — resolver continues to Base.
        assert_eq!(
            FunctionTranslator::resolve_vtable_method("Mid", "greet", &defs),
            Some("Base_greet".to_string()),
        );
    }

    #[test]
    fn resolve_vtable_method_falls_back_to_default_trait_impl() {
        let trait_with_default = trait_def("Greeter", &[], &[("greet", method(false, false))]);
        let impl_class = class("Impl", None, &["Greeter"], &[], false);
        let defs = make_defs([
            (
                "Greeter".to_string(),
                TypeDefinition::Trait(trait_with_default),
            ),
            ("Impl".to_string(), TypeDefinition::Class(impl_class)),
        ]);
        assert_eq!(
            FunctionTranslator::resolve_vtable_method("Impl", "greet", &defs),
            Some("Greeter_greet".to_string()),
        );
    }

    #[test]
    fn resolve_vtable_method_returns_none_when_method_absent() {
        let standalone = class("Standalone", None, &[], &[], false);
        let defs = make_defs([("Standalone".to_string(), TypeDefinition::Class(standalone))]);
        assert!(
            FunctionTranslator::resolve_vtable_method("Standalone", "missing", &defs).is_none()
        );
    }

    #[test]
    fn resolve_vtable_method_returns_none_for_non_class_types() {
        let defs = make_defs([(
            "AliasName".to_string(),
            TypeDefinition::Alias(crate::type_checker::context::AliasDefinition {
                template: void_type(),
                generics: None,
            }),
        )]);
        assert!(FunctionTranslator::resolve_vtable_method("AliasName", "any", &defs).is_none());
    }

    #[test]
    fn collect_vtable_methods_orders_alphabetically_and_dedups() {
        let base = class(
            "Base",
            None,
            &[],
            &[
                ("zeta", method(true, false)),
                ("alpha", method(true, false)),
            ],
            true,
        );
        let derived = class(
            "Derived",
            Some("Base"),
            &[],
            &[
                ("alpha", method(false, false)),
                ("zeta", method(false, false)),
            ],
            false,
        );
        let defs = make_defs([
            ("Base".to_string(), TypeDefinition::Class(base)),
            ("Derived".to_string(), TypeDefinition::Class(derived)),
        ]);
        let methods = FunctionTranslator::collect_vtable_methods("Derived", &defs);
        assert_eq!(methods, vec!["alpha", "zeta"]);
    }

    #[test]
    fn collect_vtable_methods_skips_constructors() {
        let base = class(
            "Base",
            None,
            &[],
            &[("init", method(true, true)), ("greet", method(true, false))],
            true,
        );
        let derived = class("Derived", Some("Base"), &[], &[], false);
        let defs = make_defs([
            ("Base".to_string(), TypeDefinition::Class(base)),
            ("Derived".to_string(), TypeDefinition::Class(derived)),
        ]);
        let methods = FunctionTranslator::collect_vtable_methods("Derived", &defs);
        assert_eq!(methods, vec!["greet"]);
    }

    #[test]
    fn collect_vtable_methods_merges_trait_required_methods() {
        let trait_def_obj = trait_def("Greeter", &[], &[("greet", method(true, false))]);
        let class_obj = class("Impl", None, &["Greeter"], &[], false);
        let defs = make_defs([
            ("Greeter".to_string(), TypeDefinition::Trait(trait_def_obj)),
            ("Impl".to_string(), TypeDefinition::Class(class_obj)),
        ]);
        let methods = FunctionTranslator::collect_vtable_methods("Impl", &defs);
        assert!(
            methods.contains(&"greet"),
            "expected 'greet' from trait, got {methods:?}",
        );
    }
}
