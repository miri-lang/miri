// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The settled facts about a program's types that a backend reads while it
//! emits code: the definition of every named type, the concrete type
//! arguments each generic class is instantiated at, and the slots each vtable
//! is filled with.
//!
//! The pipeline builds one [`TypeFacts`] after type checking and lowering have
//! settled every instantiation, and hands it to the backend read-only. A
//! backend asks it rather than reaching into the type checker or MIR lowering,
//! so the rules those layers own are answered in one place.

use crate::ast::types::Type;
use crate::mir::dispatch::VtableFills;
use std::collections::HashMap;

pub use crate::type_checker::context::{
    ClassDefinition, EnumDefinition, GenericDefinition, StructDefinition, TypeDefinition,
};

/// Read-only view of the program's types, as settled before code generation.
#[derive(Debug, Default, Clone)]
pub struct TypeFacts {
    definitions: HashMap<String, TypeDefinition>,
    generic_class_instantiations: HashMap<String, Vec<Vec<Type>>>,
    vtable_fills: VtableFills,
}

impl TypeFacts {
    /// Facts over `definitions`, where `generic_class_instantiations` lists the
    /// concrete type-argument tuples recorded for each generic class and
    /// `vtable_fills` the slots each vtable a reached body builds is filled with.
    pub fn new(
        definitions: HashMap<String, TypeDefinition>,
        generic_class_instantiations: HashMap<String, Vec<Vec<Type>>>,
        vtable_fills: VtableFills,
    ) -> Self {
        Self {
            definitions,
            generic_class_instantiations,
            vtable_fills,
        }
    }

    /// The definition of every named type, keyed by name.
    pub fn definitions(&self) -> &HashMap<String, TypeDefinition> {
        &self.definitions
    }

    /// The concrete type-argument tuples recorded for each generic class,
    /// keyed by class name.
    pub fn generic_class_instantiations(&self) -> &HashMap<String, Vec<Vec<Type>>> {
        &self.generic_class_instantiations
    }

    /// The filled slots of every vtable a reached body builds. A vtable no fill
    /// names is defined with every slot null.
    pub fn vtable_fills(&self) -> &VtableFills {
        &self.vtable_fills
    }
}
