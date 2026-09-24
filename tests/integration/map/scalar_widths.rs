// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Map operations whose key or value type is a scalar narrower or wider than
//! `int`.
//!
//! Such an instantiation compiles a per-element-width copy of the map's own
//! methods. A method that forwards a parameter straight to a runtime call —
//! `remove`, `set` — passes it through a temp typed from the runtime
//! declaration, which spells the map's type parameters. Left unsubstituted that
//! spelling is an unknown named type, so reference counting treats a plain
//! integer as a pointer and releases it.

use crate::integration::utils::*;

#[test]
fn test_map_i32_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<i32, i32>({})
    m.set(1, 10)
    m.set(2, 20)
    let removed = m.remove(1)
    println(f"{removed}")
    println(f"{m.length()}")
    println(f"{m[2]}")
"#,
        "true
1
20",
    );
}

#[test]
fn test_map_i32_remove_absent_key_is_false() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<i32, i32>({})
    m.set(1, 10)
    let removed = m.remove(9)
    println(f"{removed}")
    println(f"{m.length()}")
"#,
        "false
1",
    );
}

#[test]
fn test_map_string_key_float_value_remove() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, f64>({})
    m.set("a", 2.5)
    m.set("b", 3.5)
    let removed = m.remove("a")
    let kept = m["b"]
    println(f"{removed}")
    println(f"{kept}")
"#,
        "true
3.5",
    );
}

#[test]
fn test_map_int_remove_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map({1: 10, 2: 20})
    let removed = m.remove(1)
    println(f"{removed}")
    println(f"{m.length()}")
"#,
        "true
1",
    );
}

#[test]
fn test_map_i128_keys_that_share_a_low_word_stay_distinct() {
    // Two keys agreeing in their low eight bytes must not fold into one entry.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var m = Map<i128, int>()
    m[big] = 1
    m[twin] = 2
    println(f"{m.length()}")
    println(f"{m.get(big) ?? -1} {m.get(twin) ?? -1}")
    println(f"{m.contains_key(big)} {m.contains_key(twin)}")
"#,
        "2
1 2
true true",
    );
}

#[test]
fn test_map_i128_key_lookup_misses_a_key_never_stored() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var m = Map<i128, int>()
    m[big] = 1
    println(f"{m.contains_key(twin)} {m.get(twin) ?? -1}")
"#,
        "false -1",
    );
}

#[test]
fn test_map_i128_removes_only_the_key_asked_for() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var m = Map<i128, int>()
    m[big] = 1
    m[twin] = 2
    println(f"{m.remove(twin)} {m.length()}")
    println(f"{m.get(big) ?? -1} {m.contains_key(twin)}")
"#,
        "true 1
1 false",
    );
}

#[test]
fn test_map_u128_keys_distinguish_above_the_low_word() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let all_ones u128 = ~(0 as u128)
    let low_ones u128 = 18446744073709551615
    var m = Map<u128, int>()
    m[all_ones] = 1
    m[low_ones] = 2
    println(f"{m.length()}")
    println(f"{m.get(all_ones) ?? -1} {m.get(low_ones) ?? -1}")
"#,
        "2
1 2",
    );
}

#[test]
fn test_map_i128_overwrites_the_same_key() {
    // Passes today for the same reason its sibling does: one key, one low
    // word. It guards a widening fix against splitting one key into two.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var m = Map<i128, int>()
    m[big] = 1
    m[big] = 9
    println(f"{m.length()} {m.get(big) ?? -1}")
"#,
        "1 9",
    );
}

#[test]
fn test_map_i128_literal_keys_stay_distinct() {
    // A map built from a literal stores its entries at a second emit site, the
    // way a set literal does, so a key it writes has to be the one a later
    // lookup spells.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var m = Map<i128, int>({big: 1, twin: 2})
    println(f"{m.length()}")
    println(f"{m.get(big) ?? -1} {m.get(twin) ?? -1}")
"#,
        "2
1 2",
    );
}

#[test]
fn test_map_i128_values_leave_the_entry_table_intact() {
    // A value sixteen bytes wide travels in the argument position after the
    // key, so it is the position a width rule reading only keys would miss.
    // What can be asserted here is that the map's account of itself survives
    // storing and overwriting one: how many entries it holds, and which keys.
    //
    // Not the value bytes — an element wider than a value word still comes back
    // through one, so a wide value cannot yet be read back to be compared.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var m = Map<int, i128>()
    m[1] = big
    m[2] = twin
    println(f"{m.length()} {m.contains_key(1)} {m.contains_key(2)}")
    m[1] = twin
    println(f"{m.length()} {m.contains_key(1)} {m.contains_key(2)}")
"#,
        "2 true true
2 true true",
    );
}

#[test]
fn test_map_i128_index_read_reads_the_whole_key() {
    // An index read lowers at a seam of its own, and a compound assignment
    // through one reads and writes at that same seam.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var m = Map<i128, int>({})
    m[big] = 7
    m[twin] = 9
    println(f"{m[big]} {m[twin]}")
    m[big] += 1
    println(f"{m.length()} {m[big]} {m[twin]}")
"#,
        "7 9
2 8 9",
    );
}

#[test]
fn test_map_i128_lookup_widens_a_narrow_negative() {
    // A lookup spelled at `int` has to reach the map as the sixteen-byte key
    // the store wrote, sign bit and all: zero-filling the upper half of a
    // negative would search for a key nothing put there.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let narrow int = -3
    var m = Map<i128, int>({})
    m[-3] = 42
    println(f"{m.contains_key(narrow)} {m[narrow]} {m.get(narrow) ?? 0}")
    println(f"{m.remove(narrow)} {m.length()}")
"#,
        "true 42 42
true 0",
    );
}

#[test]
fn test_map_i128_stores_a_narrow_negative_key_at_the_slot_width() {
    // The other direction: a narrow negative key going in must be widened the
    // same way, or the wide value it equals will not find it.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let narrow int = -11
    let wide i128 = -11
    var m = Map<i128, int>({})
    m[narrow] = 4
    println(f"{m.contains_key(wide)} {m[wide]} {m.length()}")
"#,
        "true 4 1",
    );
}

#[test]
fn test_collections_of_mixed_slot_widths_verify_clean() {
    // The suite compiles with MIR verification on, so a body whose element
    // widths disagree with the slots they fill fails here rather than answering
    // a lookup wrong at runtime. A collection nested inside another is the
    // shape whose slot type has no width of its own to compare against.
    assert_runs_with_output(
        r#"
use system.collections.map
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let small i32 = 2
    var narrow = Set<i32>({})
    narrow.add(1)
    narrow.add(small)
    var text = Map<String, f64>({})
    text["x"] = 1.5
    var wide = Set<i128>({})
    wide.add(big)
    var nested = Map<int, Set<int>>({})
    nested[1] = Set<int>({2, 3})
    println(f"{narrow.length()} {text.length()} {wide.length()} {nested.length()}")
    println(f'{narrow.contains(small)} {text["x"]} {wide.contains(big)} {nested[1].contains(3)}')
"#,
        "2 1 1 1
true 1.5 true true",
    );
}

#[test]
fn test_map_u8_keys_stay_distinct() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<u8, int>()
    m.set(1, 10)
    m.set(255, 20)
    m.set(1, 30)
    println(f"{m.length()} {m[1]} {m[255]} {m.contains_key(2)}")
"#,
        "2 30 20 false",
    );
}
