// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Generic-class instantiation registry. During constructor inference the type
//! checker records, per generic class name, the set of resolved type-argument
//! tuples it is instantiated with. A later monomorphization pass consumes this
//! to emit one specialized body per distinct instantiation; recording happens
//! here so the discovery does not require a second AST walk.

use crate::type_checker::utils::type_checker_result;
use miri::ast::types::{Type, TypeKind};
use std::collections::HashMap;

fn instantiations(source: &str) -> HashMap<String, Vec<Vec<Type>>> {
    type_checker_result(source)
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

const HANDLE: &str = "class Handle\n    var id int\n    fn init(id int)\n        self.id = id\n\n";

/// The kinds `class` is recorded at, each tuple's first argument.
fn first_argument_kinds(inst: &HashMap<String, Vec<Vec<Type>>>, class: &str) -> Vec<TypeKind> {
    (inst.get(class).into_iter().flatten())
        .filter_map(|tuple| tuple.first())
        .map(|ty| ty.kind.clone())
        .collect()
}

/// The instantiations ownership is decided over, a generic body's included
/// at each type the program reaches it at.
fn ownership_instantiations(source: &str) -> HashMap<String, Vec<Vec<Type>>> {
    type_checker_result(source)
        .type_checker
        .ownership_instantiations()
        .clone()
}

fn handle_kind() -> TypeKind {
    TypeKind::Custom("Handle".to_string(), None)
}

#[test]
fn test_a_class_built_in_a_generic_function_is_recorded_at_the_type_the_call_binds() {
    let inst = ownership_instantiations(&format!(
        "{HANDLE}{BOX}fn wrap<T>(x T) Box<T>\n    return Box<T>(i: x)\n\nfn main()\n    let b = wrap(Handle(1))\n    b.unwrap()\n"
    ));
    assert!(
        first_argument_kinds(&inst, "Box").contains(&handle_kind()),
        "Box<Handle> recorded where wrap is called at Handle"
    );
}

#[test]
fn test_a_class_built_two_generic_calls_deep_is_recorded_at_the_outer_binding() {
    let inst = ownership_instantiations(&format!(
        "{HANDLE}{BOX}fn wrap<T>(x T) Box<T>\n    return Box<T>(i: x)\n\nfn through<U>(x U) Box<U>\n    return wrap(x)\n\nfn main()\n    let b = through(Handle(1))\n    b.unwrap()\n"
    ));
    assert!(
        first_argument_kinds(&inst, "Box").contains(&handle_kind()),
        "Box<Handle> recorded where through is called at Handle"
    );
}

#[test]
fn test_a_class_built_in_a_generic_function_no_one_calls_is_recorded_at_no_type() {
    let inst = ownership_instantiations(&format!(
        "{HANDLE}{BOX}fn wrap<T>(x T) Box<T>\n    return Box<T>(i: x)\n\nfn main()\n    let h = Handle(1)\n    h.id\n"
    ));
    assert!(
        !first_argument_kinds(&inst, "Box").contains(&handle_kind()),
        "no call binds T, so no concrete Box is recorded"
    );
}
