// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Generic-class instantiation registry. During constructor inference the type
//! checker records, per generic class name, the set of resolved type-argument
//! tuples it is instantiated with. A later monomorphization pass consumes this
//! to emit one specialized body per distinct instantiation; recording happens
//! here so the discovery does not require a second AST walk.

use miri::ast::types::{Type, TypeKind};
use miri::pipeline::Pipeline;
use std::collections::HashMap;

fn instantiations(source: &str) -> HashMap<String, Vec<Vec<Type>>> {
    let pipeline = Pipeline::new();
    pipeline
        .frontend(source)
        .expect("type check should succeed")
        .type_checker
        .generic_class_instantiations
}

fn arg_kinds(tuples: &[Vec<Type>]) -> Vec<Vec<TypeKind>> {
    tuples
        .iter()
        .map(|tuple| tuple.iter().map(|ty| ty.kind.clone()).collect())
        .collect()
}

const BOX: &str = "class Box<T>\n    var item T\n    fn init(i T)\n        self.item = i\n    fn unwrap() T\n        return self.item\n\n";

#[test]
fn test_box_int_and_float_are_both_recorded() {
    let inst = instantiations(&format!(
        "{BOX}fn main()\n    let a = Box<int>(i: 3)\n    let b = Box<float>(i: 2.5)\n    a.unwrap()\n    b.unwrap()\n"
    ));
    let tuples = inst.get("Box").expect("Box should be recorded");
    let kinds = arg_kinds(tuples);
    assert!(kinds.contains(&vec![TypeKind::Int]), "Box<int> recorded");
    assert!(
        kinds.contains(&vec![TypeKind::Float]),
        "Box<float> recorded"
    );
    assert_eq!(tuples.len(), 2, "exactly two distinct instantiations");
}

#[test]
fn test_repeated_instantiation_is_deduplicated() {
    let inst = instantiations(&format!(
        "{BOX}fn main()\n    let a = Box<int>(i: 1)\n    let b = Box<int>(i: 2)\n    a.unwrap()\n    b.unwrap()\n"
    ));
    let tuples = inst.get("Box").expect("Box should be recorded");
    assert_eq!(tuples.len(), 1, "Box<int> recorded once despite two sites");
    assert_eq!(arg_kinds(tuples), vec![vec![TypeKind::Int]]);
}

#[test]
fn test_non_generic_class_is_not_recorded() {
    let inst = instantiations(
        "class Point\n    var x int\n    fn init(v int)\n        self.x = v\n\nfn main()\n    let p = Point(v: 1)\n    p.x\n",
    );
    assert!(
        inst.get("Point").is_none(),
        "a class with no generics is not a monomorphization target"
    );
}
