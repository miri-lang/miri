// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The order a trait hierarchy is searched in.

use miri::type_checker::context::{trait_lineage, TraitDefinition, TypeDefinition};
use std::collections::{BTreeMap, HashMap};

fn trait_def(name: &str, parents: &[&str]) -> (String, TypeDefinition) {
    (
        name.to_string(),
        TypeDefinition::Trait(TraitDefinition {
            name: name.to_string(),
            generics: None,
            parent_traits: parents.iter().map(|p| p.to_string()).collect(),
            parent_trait_args: BTreeMap::new(),
            methods: BTreeMap::new(),
            module: String::new(),
        }),
    )
}

#[test]
fn trait_lineage_asks_a_trait_before_its_parents_in_the_order_it_lists_them() {
    let defs: HashMap<String, TypeDefinition> = [
        trait_def("Both", &["Left", "Right"]),
        trait_def("Left", &["Root"]),
        trait_def("Right", &["Root"]),
        trait_def("Root", &[]),
    ]
    .into_iter()
    .collect();
    let order: Vec<&str> = trait_lineage(&defs, "Both").map(|(name, _)| name).collect();
    assert_eq!(order, ["Both", "Left", "Right", "Root"]);
}
