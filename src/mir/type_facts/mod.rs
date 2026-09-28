// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The settled facts about a program's types that a backend reads while it
//! emits code: the definition of every named type, the instantiations whose
//! method bodies were lowered, the instantiations a value is released at, the
//! slots each vtable is filled with, and the answers the dispatch rules give
//! for a type — its drop hook, its `clone`, the traits it implements and the
//! methods a container asks of its elements.
//!
//! The pipeline builds one [`TypeFacts`] after type checking and lowering have
//! settled every instantiation, and hands it to the backend read-only. A
//! backend asks it rather than reaching into the type checker or MIR lowering,
//! so each rule those layers own is answered by its one implementation.

mod drop_instantiations;

use crate::ast::expression::Expression;
use crate::ast::types::Type;
use crate::mir::dispatch::VtableFills;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::Body;
use std::collections::{HashMap, HashSet};

pub use crate::type_checker::context::{
    ClassDefinition, EnumDefinition, FieldInfo, GenericDefinition, MethodInfo, StructDefinition,
    TypeDefinition,
};

/// Why the instantiations a drop thunk is emitted for could not be closed.
#[derive(Debug, Clone, PartialEq)]
pub enum DropInstantiationRefusal {
    /// `name` at `args` nests its type arguments `depth` constructors deep,
    /// past the depth any instance may nest to: a field that nests its own
    /// type deeper on every instantiation.
    TooDeep {
        name: String,
        args: Vec<Type>,
        depth: usize,
    },
    /// Two instantiations of `name` at types with no name would share the
    /// drop thunk `symbol`, laid out for only one of them.
    Unnameable { name: String, symbol: String },
}

/// Read-only view of the program's types, as settled before code generation.
#[derive(Debug, Default, Clone)]
pub struct TypeFacts {
    definitions: HashMap<String, TypeDefinition>,
    generic_class_instantiations: HashMap<String, Vec<Vec<Type>>>,
    drop_instantiations: HashMap<String, Vec<Vec<Type>>>,
    vtable_fills: VtableFills,
    withheld_methods: HashSet<String>,
}

impl TypeFacts {
    /// Facts over `definitions`, where `generic_class_instantiations` lists the
    /// instantiations whose method bodies were lowered, `vtable_fills` the
    /// slots each vtable a reached body builds is filled with, and `bodies`
    /// every body the backend compiles — whose held values decide the
    /// instantiations a drop thunk is emitted for.
    ///
    /// Refused when those instantiations cannot be closed: a field nests its
    /// own type deeper on every instantiation, or two instantiations would
    /// share one drop thunk.
    pub fn new<'b>(
        definitions: HashMap<String, TypeDefinition>,
        generic_class_instantiations: HashMap<String, Vec<Vec<Type>>>,
        vtable_fills: VtableFills,
        bodies: impl IntoIterator<Item = &'b Body>,
    ) -> Result<Self, DropInstantiationRefusal> {
        let drop_instantiations = drop_instantiations::drop_instantiations(
            &definitions,
            &generic_class_instantiations,
            bodies,
        )?;
        Ok(Self {
            definitions,
            generic_class_instantiations,
            drop_instantiations,
            vtable_fills,
            withheld_methods: HashSet::new(),
        })
    }

    /// The same facts, with `withheld_methods` the link names of the methods
    /// lowering withheld at an instance nothing that runs reaches them at.
    pub fn withholding(mut self, withheld_methods: HashSet<String>) -> Self {
        self.withheld_methods = withheld_methods;
        self
    }

    /// The definition of every named type, keyed by name.
    pub fn definitions(&self) -> &HashMap<String, TypeDefinition> {
        &self.definitions
    }

    /// The concrete type-argument tuples each generic class's method bodies
    /// were lowered at, keyed by class name.
    pub fn generic_class_instantiations(&self) -> &HashMap<String, Vec<Vec<Type>>> {
        &self.generic_class_instantiations
    }

    /// The type-argument tuples a drop thunk is emitted for, for the generic
    /// type `name`: every instantiation a compiled body holds a value of, and
    /// every one a field or payload of those stores. Empty for a type no value
    /// is released at.
    pub fn drop_instantiations_of(&self, name: &str) -> &[Vec<Type>] {
        self.drop_instantiations
            .get(name)
            .map_or(&[], Vec::as_slice)
    }

    /// Whether a drop thunk is emitted for `name` at `args`, compared by the
    /// symbol both sides name it through.
    pub fn is_drop_instantiation(&self, name: &str, args: &[Type]) -> bool {
        let wanted = Symbol::type_thunk(ThunkKind::Drop, name, args);
        self.drop_instantiations_of(name)
            .iter()
            .any(|tuple| Symbol::type_thunk(ThunkKind::Drop, name, tuple) == wanted)
    }

    /// The name of every type parameter `ty` leaves open, anywhere inside it.
    pub fn open_parameter_names(&self, ty: &Type) -> Vec<String> {
        crate::mir::instantiation::shape::open_parameter_names(ty, &self.definitions)
    }

    /// The filled slots of every vtable a reached body builds. A vtable no fill
    /// names is defined with every slot null.
    pub fn vtable_fills(&self) -> &VtableFills {
        &self.vtable_fills
    }

    /// Whether lowering withheld the method body `link_name` names: it
    /// compiled no body for it, so nothing may call or register it.
    pub fn is_withheld(&self, link_name: &str) -> bool {
        self.withheld_methods.contains(link_name)
    }

    /// The type each field of the struct or class `name` is stored at in an
    /// instance carrying `args`, in layout order; `None` when `name` names no
    /// struct or class. Every read of a field type at an instantiation asks
    /// here, so layout, drop and projection agree on what a field holds.
    pub fn field_types(&self, name: &str, args: Option<&[Expression]>) -> Option<Vec<Type>> {
        crate::mir::instantiation::field_types(&self.definitions, name, args)
    }

    /// Whether releasing a `name` value runs a `fn drop(self)` hook, declared
    /// on the type itself, inherited from a base class or supplied by a trait.
    pub fn has_drop_hook(&self, name: &str) -> bool {
        crate::type_checker::utils::has_drop_hook(name, &self.definitions)
    }

    /// The symbol of the drop hook releasing a `name` value runs, or `None`
    /// when the type has none.
    pub fn drop_hook_symbol(&self, name: &str) -> Option<String> {
        crate::mir::lowering::dispatch_symbols::drop_hook_symbol(name, &self.definitions)
    }

    /// The symbol of the `clone` a copy of a `name` element calls.
    pub fn clone_symbol(&self, name: &str) -> String {
        crate::mir::lowering::dispatch_symbols::clone_method_symbol(name, &self.definitions)
    }

    /// Whether the class `name`, or a class it extends, implements
    /// `trait_name`, directly or through a trait that extends it.
    pub fn implements(&self, name: &str, trait_name: &str) -> bool {
        crate::type_checker::context::class_implements_trait(name, trait_name, &self.definitions)
    }

    /// The method `method_name` a call on a `name` receiver reaches along its
    /// class chain or trait hierarchy, as static dispatch resolves it.
    pub fn inherited_method(&self, name: &str, method_name: &str) -> Option<MethodInfo> {
        crate::mir::lowering::method_dispatch::resolve_inherited_method(
            &self.definitions,
            name,
            method_name,
        )
        .map(|(_, method)| method)
    }

    /// The class in `name`'s chain that declares `method_name`, with the
    /// declaration, nearest first.
    pub fn class_method(&self, name: &str, method_name: &str) -> Option<(&str, &MethodInfo)> {
        crate::type_checker::context::class_method_declaration(name, method_name, &self.definitions)
    }

    /// The symbol of the body a call to `method_name` on a `name` element
    /// reaches: the body compiled for the instantiation `args` where one is,
    /// else the shared one static dispatch names.
    pub fn element_method_symbol(
        &self,
        name: &str,
        method_name: &str,
        args: Option<&[Type]>,
    ) -> String {
        args.and_then(|args| {
            crate::mir::lowering::method_dispatch::instantiated_callee(
                &self.definitions,
                name,
                args,
                method_name,
            )
        })
        .map(|callee| callee.symbol)
        .unwrap_or_else(|| {
            crate::mir::lowering::dispatch_symbols::method_symbol(
                &self.definitions,
                name,
                method_name,
            )
        })
    }
}
