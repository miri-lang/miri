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
#[ignore = "a map key wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the key as a pointer-sized integer and copies key_size bytes from the address of that stack parameter, so two keys sharing a low word fold into one entry. Store and lookup are wrong together, so no partial fix helps"]
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
#[ignore = "a map key wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the key as a pointer-sized integer and copies key_size bytes from the address of that stack parameter, so two keys sharing a low word fold into one entry. Store and lookup are wrong together, so no partial fix helps"]
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
#[ignore = "a map key wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the key as a pointer-sized integer and copies key_size bytes from the address of that stack parameter, so two keys sharing a low word fold into one entry. Store and lookup are wrong together, so no partial fix helps"]
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
#[ignore = "a map key wider than a value word is truncated to its low eight bytes at the runtime call: the entry point takes the key as a pointer-sized integer and copies key_size bytes from the address of that stack parameter, so two keys sharing a low word fold into one entry. Store and lookup are wrong together, so no partial fix helps"]
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
