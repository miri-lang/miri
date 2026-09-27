// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which of a class's own method bodies the type checker records as settled by
//! the runtime. The rule is stated once, in `src/type_checker/runtime_settled.rs`.
//!
//! The set is recorded for every class, but today only a built-in
//! collection's set is read — by dispatch, to share one lowering of a settled
//! body. The user classes below exercise the rule, not a consumer of it.

use crate::type_checker::utils::type_checker_result;
use miri::type_checker::context::TypeDefinition;
use std::collections::BTreeSet;

fn settled_methods(source: &str, class_name: &str) -> BTreeSet<String> {
    let result = type_checker_result(source);
    match result.type_checker.type_definitions().get(class_name) {
        Some(TypeDefinition::Class(class_def)) => class_def.runtime_settled_methods.clone(),
        other => panic!("`{class_name}` is not a class: {other:?}"),
    }
}

fn assert_settled(settled: &BTreeSet<String>, class_name: &str, methods: &[&str]) {
    for method in methods {
        assert!(
            settled.contains(*method),
            "`{class_name}.{method}` should be settled by the runtime; settled: {settled:?}"
        );
    }
}

fn assert_unsettled(settled: &BTreeSet<String>, class_name: &str, methods: &[&str]) {
    for method in methods {
        assert!(
            !settled.contains(*method),
            "`{class_name}.{method}` should not be settled by the runtime; settled: {settled:?}"
        );
    }
}

#[test]
fn list_pairs_its_element_takes_with_the_runtime_but_not_its_lookups() {
    let settled = settled_methods(
        "use system.collections.list\nfn main()\n    let l = List<int>()\n",
        "List",
    );
    assert_settled(
        &settled,
        "List",
        &["length", "pop", "remove_at", "last_index", "push", "clone"],
    );
    // `remove` finds its element through the `index_of` trait default, which
    // compares with `==`.
    assert_unsettled(&settled, "List", &["remove"]);
}

#[test]
fn map_set_and_array_keep_their_runtime_backed_methods_settled() {
    let source = "use system.collections.list\nuse system.collections.map\nuse system.collections.set\nuse system.collections.array\nfn main()\n    let l = List<int>()\n";
    let result = type_checker_result(source);
    let settled_of = |name: &str| match result.type_checker.type_definitions().get(name) {
        Some(TypeDefinition::Class(class_def)) => class_def.runtime_settled_methods.clone(),
        other => panic!("`{name}` is not a class: {other:?}"),
    };
    let map = settled_of("Map");
    assert_settled(&map, "Map", &["get", "is_empty", "remove"]);
    assert_unsettled(&map, "Map", &["filter", "map", "reduce"]);
    let set = settled_of("Set");
    assert_settled(&set, "Set", &["is_empty", "contains"]);
    assert_unsettled(&set, "Set", &["filter"]);
    let array = settled_of("Array");
    assert_settled(&array, "Array", &["length", "sort"]);
    // `reverse` swaps elements with plain stores, which the shared body would
    // make without taking or releasing a reference.
    assert_unsettled(&array, "Array", &["reverse"]);
}

#[test]
fn an_operator_on_a_type_parameter_value_is_not_settled_by_the_runtime() {
    let settled = settled_methods(
        "class Pair<T>\n    a T\n    b T\n    fn init(a T, b T)\n        self.a = a\n        self.b = b\n    fn same() bool: self.a == self.b\n    fn asks_same() bool: self.same()\n    fn count() int: 2\n    fn doubled() int: self.count() * 2\n\nfn main()\n    let p = Pair<int>(1, 2)\n",
        "Pair",
    );
    assert_settled(&settled, "Pair", &["count", "doubled"]);
    // `asks_same` only calls a method of its own object, but that method is
    // itself unsettled, so the fixpoint removes it too.
    assert_unsettled(&settled, "Pair", &["same", "asks_same"]);
}

/// A generic class `Bag<T>` holding `items List<T>` and one `T`, with `probe`
/// appended as the method under test.
fn bag_settled_methods(probe: &str) -> BTreeSet<String> {
    settled_methods(
        &format!(
            "use system.collections.list\nclass Bag<T>\n    items List<T>\n    one T\n    fn init(items List<T>, one T)\n        self.items = items\n        self.one = one\n    fn size() int: 2\n{probe}\nfn main()\n    let b = Bag<int>(List<int>(), 1)\n"
        ),
        "Bag",
    )
}

#[test]
fn a_store_of_a_type_parameter_value_is_not_settled_by_the_runtime() {
    let settled = bag_settled_methods("    fn put(x T)\n        self.one = x\n");
    assert_settled(&settled, "Bag", &["size"]);
    assert_unsettled(&settled, "Bag", &["put", "init"]);
}

#[test]
fn a_loop_over_a_type_parameter_collection_is_not_settled_by_the_runtime() {
    let settled = bag_settled_methods(
        "    fn walk() int\n        var n = 0\n        for x in self.items\n            n = n + 1\n        return n\n    fn count_to(k int) int\n        var n = 0\n        for i in 0..k\n            n = n + 1\n        return n\n",
    );
    assert_settled(&settled, "Bag", &["count_to"]);
    assert_unsettled(&settled, "Bag", &["walk"]);
}

#[test]
fn a_match_that_binds_a_type_parameter_value_is_not_settled_by_the_runtime() {
    let settled = bag_settled_methods(
        "    fn has(x T?) bool\n        match x\n            Some(v): true\n            None: false\n",
    );
    assert_unsettled(&settled, "Bag", &["has"]);
}

#[test]
fn indexing_a_type_parameter_collection_other_than_self_is_not_settled_by_the_runtime() {
    let settled = bag_settled_methods(
        "    fn first_of(xs List<T>) T: xs[0]\n    fn nth_size(xs List<int>) int: xs[0]\n",
    );
    assert_settled(&settled, "Bag", &["nth_size"]);
    assert_unsettled(&settled, "Bag", &["first_of"]);
}
