// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A drop thunk counts its release depth exactly when its plan says the type
//! can recurse: a flat class pays nothing, a recursive one is bounded.

use super::*;
use crate::ast::types::Type;
use crate::ast::MemberVisibility;
use crate::mir::dispatch::VtableFills;
use crate::mir::type_facts::{ClassDefinition, FieldInfo};
use cranelift_module::{default_libcall_names, Module};
use cranelift_object::ObjectBuilder;
use std::collections::BTreeSet;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Default::default())
}

fn class(name: &str, field: TypeKind) -> (String, TypeDefinition) {
    let definition = ClassDefinition {
        name: name.to_string(),
        generics: None,
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![(
            "field".to_string(),
            FieldInfo {
                ty: ty(field),
                mutable: true,
                visibility: MemberVisibility::Public,
                initializer: None,
            },
        )],
        methods: BTreeMap::new(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    };
    (name.to_string(), TypeDefinition::Class(definition))
}

fn facts() -> TypeFacts {
    let node_field = TypeKind::Option(Box::new(ty(TypeKind::Custom("Node".to_string(), None))));
    let definitions = HashMap::from([class("Flat", TypeKind::String), class("Node", node_field)]);
    match TypeFacts::new(definitions, HashMap::new(), VtableFills::default(), []) {
        Ok(facts) => facts,
        Err(refusal) => panic!("the facts settle: {refusal:?}"),
    }
}

/// Whether emitting the drop thunk of `name` declares the depth counter: each
/// emission gets a fresh module, so the name appears only if this thunk uses it.
fn declares_the_depth_counter(name: &str) -> bool {
    let flags = cranelift_codegen::settings::Flags::new(cranelift_codegen::settings::builder());
    let isa = match cranelift_native::builder().and_then(|b| b.finish(flags).map_err(|_| "isa")) {
        Ok(isa) => isa,
        Err(error) => panic!("the host ISA builds: {error}"),
    };
    let builder = match ObjectBuilder::new(isa.clone(), "thunk", default_libcall_names()) {
        Ok(builder) => builder,
        Err(error) => panic!("an object module builds: {error}"),
    };
    let mut module = ObjectModule::new(builder);
    let mut ctx = module.make_context();
    if let Err(error) = FunctionTranslator::generate_drop_function(
        &mut module,
        &mut ctx,
        &isa,
        name,
        None,
        &facts(),
    ) {
        panic!("the drop thunk of {name} emits: {error}");
    }
    module.get_name(rt::RELEASE_DEPTH).is_some()
}

#[test]
fn a_flat_class_counts_no_release_depth() {
    assert!(!declares_the_depth_counter("Flat"));
}

#[test]
fn a_recursive_class_counts_its_release_depth() {
    assert!(declares_the_depth_counter("Node"));
}
