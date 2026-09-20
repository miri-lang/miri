// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a map decides that two keys are the same key.

use super::utils::*;

#[test]
#[ignore = "an optional element or key is matched by the address of its Some box, so two equal optionals are stored as two entries and a lookup misses what the container holds. The container asks a rule the compiler registers per element type — by bytes, by string content, or through a generated equals callback — and an optional gets none of them, so it falls to comparing bytes, which for an optional are a pointer. `==` on two optionals is already correct, so the semantics exist; what is missing is a way for the container to reach them"]
fn map_with_optional_keys_matches_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, String>()
    m.set(Some(2), "two")
    let two = m.get(Some(2)) ?? "none"
    let has = m.contains_key(Some(2))
    println(f"{m.length()} {two} {has}")
"#,
        "1 two true",
    );
}

#[test]
#[ignore = "an optional element is matched by the address of its Some box, so a set holds two entries for one value; see the sibling map test for the mechanism"]
fn set_of_optionals_deduplicates_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<int?>()
    s.add(Some(2))
    s.add(Some(2))
    let has = s.contains(Some(2))
    println(f"{s.length()} {has}")
"#,
        "1 true",
    );
}

#[test]
#[ignore = "two None optionals are matched by address like any other optional, so a set holds two of them; see the sibling map test for the mechanism"]
fn set_of_optionals_treats_none_as_one_value() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<int?>()
    s.add(None)
    s.add(None)
    let has = s.contains(None)
    println(f"{s.length()} {has}")
"#,
        "1 true",
    );
}

#[test]
fn equality_on_optionals_is_already_correct() {
    // The rule a container would have to apply exists and works when written by
    // hand. This is what makes the container case a matter of reaching those
    // semantics rather than deciding them.
    assert_runs_with_output(
        r#"
fn main()
    let a int? = Some(2)
    let b int? = Some(2)
    let c int? = None
    let d int? = None
    let e int? = Some(3)
    println(f"{a == b} {c == d} {a == e} {a == c}")
"#,
        "true true false false",
    );
}
