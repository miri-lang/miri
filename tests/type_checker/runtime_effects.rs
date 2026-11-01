// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a `runtime` declaration states its entry point does.

use std::collections::BTreeSet;

use crate::type_checker::utils::{type_checker_error_test, type_checker_result};
use miri::mir::compiler_intrinsics::{compiler_intrinsic, compiler_intrinsic_names};
use miri::runtime_effects::{ParamUse, RuntimeEffect};
use miri::runtime_fns as table;

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

/// Where the record of `name` answers differently from the name tables it
/// replaces, one line per disagreement.
fn disagreements(name: &str, effect: &RuntimeEffect) -> Vec<String> {
    let mut found = Vec::new();
    let mut differ = |what: &str, stated: bool, tabled: bool| {
        if stated != tabled {
            found.push(format!("{name} {what}: stated {stated}, table {tabled}"));
        }
    };
    for position in 0..effect.params.len() {
        if effect.param(position) == ParamUse::Scalar {
            continue;
        }
        let at = |what: &str| format!("{what}({position})");
        differ(
            &at("only_reads"),
            effect.only_reads(position),
            table::only_reads_argument(name, position),
        );
        differ(
            &at("writes_in_place"),
            effect.writes_in_place(position),
            table::writes_in_place(name, position),
        );
        differ(
            &at("taken"),
            effect.taken_positions().contains(&position),
            table::taken_argument_positions(name).contains(&position),
        );
        differ(
            &at("closure"),
            effect.closure_contract(position).is_some(),
            table::closure_contract(name, position).is_some(),
        );
    }
    differ(
        "element_roles",
        true,
        effect.element_roles() == table::element_roles(name),
    );
    differ(
        "copy_on_write",
        effect.is_copy_on_write(),
        table::is_copy_on_write(name),
    );
    differ(
        "returns_copy",
        effect.returns_copy(),
        table::returns_unshared(name),
    );
    differ(
        "allocates",
        effect.allocates_collection(),
        table::allocates_collection(name),
    );
    differ(
        "preserves_length",
        effect.preserves_length(),
        table::preserves_length(name),
    );
    differ(
        "copies_collection",
        effect.copies_collection(),
        table::copies_collection(name),
    );
    differ(
        "orders",
        effect.orders.is_some(),
        table::orders_its_elements(name),
    );
    differ(
        "matches",
        effect.matches.is_some(),
        table::matches_its_elements(name),
    );
    differ(
        "returns_element",
        effect.returns_element_value(),
        table::returns_element_value(name),
    );
    differ(
        "borrowed",
        effect.hands_back_a_borrow(),
        table::hands_back_a_borrow(name),
    );
    differ("diverges", effect.diverges, table::diverges(name));
    found
}

/// The declarations state what the name tables said, except where the tables
/// were wrong or needlessly cautious:
///
/// - `list_set` and `array_set_val` keep the element they are handed, as
///   `list_push` does; the table omitted them because no compiled call reached
///   them.
/// - `map_remove` and `set_remove` only read the key they look up, as
///   `map_contains_key` does.
/// - A map or a set clone is a copy as long as its original, as a list or an
///   array clone is.
#[test]
fn the_declarations_state_what_the_name_tables_say() {
    let found: BTreeSet<String> = declared()
        .iter()
        .flat_map(|(name, effect)| disagreements(name, effect))
        .collect();
    let expected: BTreeSet<String> = [
        "miri_rt_array_set_val taken(2): stated true, table false",
        "miri_rt_list_set taken(2): stated true, table false",
        "miri_rt_map_remove only_reads(1): stated true, table false",
        "miri_rt_set_remove only_reads(1): stated true, table false",
        "miri_rt_map_clone preserves_length: stated true, table false",
        "miri_rt_map_clone copies_collection: stated true, table false",
        "miri_rt_set_clone preserves_length: stated true, table false",
        "miri_rt_set_clone copies_collection: stated true, table false",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(found, expected);
}

/// The compiler's own records state what the name tables said, except that a
/// map or a set copy-on-write hands back a collection as long as the one it
/// is handed, as a list or an array copy-on-write does.
#[test]
fn the_compiler_owned_records_state_what_the_name_tables_say() {
    let found: BTreeSet<String> = compiler_intrinsic_names()
        .filter_map(|name| Some((name, compiler_intrinsic(name)?.effect())))
        .flat_map(|(name, effect)| disagreements(name, &effect))
        .collect();
    let expected: BTreeSet<String> = [
        "miri_rt_map_cow preserves_length: stated true, table false",
        "miri_rt_map_cow copies_collection: stated true, table false",
        "miri_rt_set_cow preserves_length: stated true, table false",
        "miri_rt_set_cow copies_collection: stated true, table false",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    assert_eq!(found, expected);
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
