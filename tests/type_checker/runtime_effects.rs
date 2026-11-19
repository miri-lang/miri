// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a `runtime` declaration states its entry point does.

use std::collections::BTreeSet;

use crate::type_checker::utils::{type_checker_error_test, type_checker_result};
use miri::mir::compiler_intrinsics::compiler_intrinsic;
use miri::runtime_effects::RuntimeEffect;

/// A program importing every stdlib module that declares runtime entry
/// points, so the check records each one.
const EVERY_RUNTIME_MODULE: &str = "
use system.io
use system.time
use system.os
use system.process
use system.gpu
use system.text
use system.fs
use system.testing
use system.ops
use system.string
use system.collections.list
use system.collections.set
use system.collections.map
use system.collections.array
use system.memory

fn main()
    return
";

#[test]
fn a_declaration_that_states_nothing_about_a_reference_is_refused() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    runtime "core" fn miri_rt_x(l Self, item T)

fn main()
    return
"#,
        "states nothing about what it does with `l`",
    );
}

#[test]
fn a_result_of_a_type_parameter_must_be_stated() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    @reads("l")
    runtime "core" fn miri_rt_x(l Self) T

fn main()
    return
"#,
        "states nothing about what it hands back",
    );
}

#[test]
fn stores_on_a_parameter_that_is_no_element_is_refused() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    @writes("l")
    @stores("l")
    runtime "core" fn miri_rt_x(l Self)

fn main()
    return
"#,
        "`@stores` names `l`",
    );
}

#[test]
fn clones_and_stores_on_one_parameter_are_refused() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    @writes("l")
    @element("item")
    @stores("item")
    @clones("item")
    runtime "core" fn miri_rt_x(l Self, item T)

fn main()
    return
"#,
        "`@clones` names `item`, which is not an `@element`, `@key` or `@value` the entry point only reads",
    );
}

#[test]
fn clones_naming_no_parameter_is_refused() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    @writes("l")
    @element("item")
    @clones("thing")
    runtime "core" fn miri_rt_x(l Self, item T)

fn main()
    return
"#,
        "`miri_rt_x` has no parameter `thing`",
    );
}

#[test]
fn clones_naming_one_parameter_twice_is_refused() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    @writes("l")
    @element("item")
    @clones("item, item")
    runtime "core" fn miri_rt_x(l Self, item T)

fn main()
    return
"#,
        "`@clones` names a parameter of `miri_rt_x` twice",
    );
}

#[test]
fn two_uses_stated_for_one_parameter_are_refused() {
    type_checker_error_test(
        r#"
class Bag<T>
    var n int
    @reads("l")
    @writes("l")
    runtime "core" fn miri_rt_x(l Self)

fn main()
    return
"#,
        "states both `@reads` and `@writes` for `l`",
    );
}

#[test]
fn an_attribute_naming_no_parameter_is_refused() {
    type_checker_error_test(
        r#"
fn main()
    @reads("missing")
    runtime "core" fn miri_rt_x(n int)
"#,
        "`miri_rt_x` has no parameter `missing`",
    );
}

/// Every runtime entry point the stdlib declares, with what its declaration
/// states.
fn declared() -> Vec<(String, RuntimeEffect)> {
    let result = type_checker_result(EVERY_RUNTIME_MODULE);
    let mut declared: Vec<(String, RuntimeEffect)> = (result.type_checker.runtime_effects.iter())
        .map(|(name, effect)| (name.clone(), (**effect).clone()))
        .collect();
    declared.sort_by(|a, b| a.0.cmp(&b.0));
    declared
}

/// An entry point is described in one place: no entry point the compiler
/// describes is also declared by the stdlib.
#[test]
fn no_entry_point_is_both_declared_and_compiler_owned() {
    let both: Vec<String> = declared()
        .into_iter()
        .map(|(name, _)| name)
        .filter(|name| compiler_intrinsic(name).is_some())
        .collect();
    assert!(both.is_empty(), "described twice: {both:?}");
}

/// Every entry point the stdlib declares has a record.
#[test]
fn every_declared_entry_point_has_a_record() {
    let recorded: BTreeSet<String> = declared().into_iter().map(|(name, _)| name).collect();
    let stdlib = concat!(env!("CARGO_MANIFEST_DIR"), "/src/stdlib");
    let mut missing = Vec::new();
    for entry in walk(std::path::Path::new(stdlib)) {
        let text = std::fs::read_to_string(&entry).unwrap_or_default();
        for line in text.lines() {
            let Some(rest) = line.trim().strip_prefix("runtime ") else {
                continue;
            };
            let Some(name) = rest
                .split("fn ")
                .nth(1)
                .and_then(|after| after.split(['(', '<']).next())
            else {
                continue;
            };
            if !recorded.contains(name) {
                missing.push(name.to_string());
            }
        }
    }
    assert!(missing.is_empty(), "no record for {missing:?}");
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else if path.extension().is_some_and(|ext| ext == "mi") {
            found.push(path);
        }
    }
    found
}

#[test]
fn a_free_runtime_declaration_may_take_type_parameters() {
    crate::type_checker::utils::type_checker_test(
        r#"
@element("item")
@returns_element
runtime "core" fn miri_rt_x<T>(item T) T

fn main()
    return
"#,
    );
}

#[test]
fn a_result_of_a_declaration_type_parameter_must_be_stated() {
    type_checker_error_test(
        r#"
@element("item")
runtime "core" fn miri_rt_x<T>(item T) T

fn main()
    return
"#,
        "states nothing about what it hands back",
    );
}

#[test]
fn a_runtime_declaration_type_parameter_is_not_seen_outside_it() {
    type_checker_error_test(
        r#"
@element("item")
@returns_element
runtime "core" fn miri_rt_x<T>(item T) T

fn f(v T)
    return

fn main()
    return
"#,
        "Unknown type: T",
    );
}

#[test]
fn a_call_infers_the_type_parameters_of_a_runtime_declaration() {
    type_checker_error_test(
        r#"
@element("item")
@returns_element
runtime "core" fn miri_rt_x<T>(item T) T

fn main()
    let y = miri_rt_x("a")
    let z int = y
"#,
        "expected int, got String",
    );
}

#[test]
fn an_element_the_runtime_writes_is_stated_with_writes() {
    crate::type_checker::utils::type_checker_test(
        r#"
@element("place, new")
@writes("place")
@stores("new")
@returns_element
runtime "core" fn miri_rt_x<T>(place out T, new T) T

fn main()
    return
"#,
    );
}

#[test]
fn an_element_the_runtime_writes_must_be_an_out_parameter() {
    type_checker_error_test(
        r#"
@element("place, new")
@writes("place")
@stores("new")
@returns_element
runtime "core" fn miri_rt_x<T>(place T, new T) T

fn main()
    return
"#,
        "writes the element `place`, so it must be declared `out`",
    );
}

#[test]
fn a_value_of_the_declarations_own_type_parameter_is_no_maps_key() {
    type_checker_error_test(
        r#"
@key("item")
runtime "core" fn miri_rt_x<T>(item T) bool

fn main()
    return
"#,
        "which is a key or value of no map: name it in `@element`",
    );
}

#[test]
fn a_parameter_of_the_declarations_own_type_parameter_must_be_an_element() {
    type_checker_error_test(
        r#"
@reads("x")
runtime "core" fn miri_rt_x<T>(x T) bool

fn main()
    return
"#,
        "`miri_rt_x` takes `x` at its own type parameter, which is one word only at some types: name it in `@element`",
    );
}

#[test]
fn a_collection_and_a_closure_of_the_declarations_own_type_parameter_are_words() {
    crate::type_checker::utils::type_checker_test(
        r#"
@reads("xs")
@closure("f, 1, xs")
runtime "core" fn miri_rt_x<T>(xs [T], f fn(T) int) bool

fn main()
    return
"#,
    );
}

#[test]
fn an_optional_of_the_declarations_own_type_parameter_must_be_an_element() {
    type_checker_error_test(
        r#"
@reads("x")
runtime "core" fn miri_rt_x<T>(x T?) bool

fn main()
    return
"#,
        "`miri_rt_x` takes `x` at its own type parameter",
    );
}

#[test]
fn a_tuple_holding_the_declarations_own_type_parameter_must_be_an_element() {
    type_checker_error_test(
        r#"
@reads("x")
runtime "core" fn miri_rt_x<T>(x (T, int)) bool

fn main()
    return
"#,
        "`miri_rt_x` takes `x` at its own type parameter",
    );
}

#[test]
fn a_struct_holding_the_declarations_own_type_parameter_must_be_an_element() {
    type_checker_error_test(
        r#"
struct Pair<T>
    a T
    b T

@reads("x")
runtime "core" fn miri_rt_x<T>(x Pair<T>) bool

fn main()
    return
"#,
        "`miri_rt_x` takes `x` at its own type parameter",
    );
}

#[test]
fn a_class_instance_of_the_declarations_own_type_parameter_is_a_word() {
    crate::type_checker::utils::type_checker_test(
        r#"
class Box<T>
    var v T

@reads("x")
runtime "core" fn miri_rt_x<T>(x Box<T>) bool

fn main()
    return
"#,
    );
}

#[test]
fn a_generic_enum_of_the_declarations_own_type_parameter_is_a_word() {
    crate::type_checker::utils::type_checker_test(
        r#"
enum Slot<T>
    Full(T)
    Empty

@reads("x")
runtime "core" fn miri_rt_x<T>(x Slot<T>) bool

fn main()
    return
"#,
    );
}

#[test]
fn a_struct_counted_whatever_the_declarations_own_type_parameter_is_a_word() {
    crate::type_checker::utils::type_checker_test(
        r#"
struct Named<T>
    name String
    v T

@reads("x")
runtime "core" fn miri_rt_x<T>(x Named<T>) bool

fn main()
    return
"#,
    );
}

#[test]
fn a_result_of_the_declarations_own_type_parameter_must_be_an_element() {
    type_checker_error_test(
        r#"
@element("x")
@copies("x")
runtime "core" fn miri_rt_x<T>(x T) T?

fn main()
    return
"#,
        "`miri_rt_x` hands back its own type parameter, which is one word only at some types: state `@returns_element`",
    );
}

#[test]
fn an_element_of_the_declarations_own_type_parameter_is_accepted() {
    crate::type_checker::utils::type_checker_test(
        r#"
@element("x")
runtime "core" fn miri_rt_x<T>(x T) bool

fn main()
    return
"#,
    );
}

#[test]
fn an_unstated_result_built_from_the_declarations_own_type_parameter_is_refused() {
    type_checker_error_test(
        r#"
@element("x")
runtime "core" fn miri_rt_x<T>(x T) (T, int)

fn main()
    return
"#,
        "`miri_rt_x` hands back its own type parameter",
    );
}

#[test]
fn a_key_the_runtime_writes_is_refused() {
    type_checker_error_test(
        r#"
@key("k")
@writes("k")
runtime "core" fn miri_rt_x(k out String)

fn main()
    return
"#,
        "`miri_rt_x` states both `@key` and `@writes` for `k`",
    );
}

#[test]
fn an_out_parameter_the_runtime_does_not_overwrite_is_refused() {
    type_checker_error_test(
        r#"
@writes("s")
runtime "core" fn miri_rt_x(s out String)

fn main()
    return
"#,
        "`miri_rt_x` takes `s` `out`, but only a place it writes new bytes over, an `@element` it `@writes`, is handed to the runtime `out`",
    );
}

#[test]
fn an_out_scalar_of_a_runtime_declaration_is_refused() {
    type_checker_error_test(
        r#"
runtime "core" fn miri_rt_x(n out int)

fn main()
    return
"#,
        "`miri_rt_x` takes `n` `out`",
    );
}
