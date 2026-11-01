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
                .and_then(|after| after.split('(').next())
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
