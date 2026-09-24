// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Set operations whose element type is a scalar narrower or wider than `int`,
//! which compiles a per-element-width copy of the set's own methods.

use crate::integration::utils::*;

#[test]
fn test_set_i32_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<i32>({})
    s.add(200)
    s.add(201)
    let removed = s.remove(200)
    println(f"{removed}")
    println(f"{s.length()}")
    println(f"{s.contains(201)}")
"#,
        "true
1
true",
    );
}

#[test]
fn test_set_f64_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<f64>({})
    s.add(1.5)
    s.add(2.5)
    let removed = s.remove(1.5)
    println(f"{removed}")
    println(f"{s.length()}")
    println(f"{s.contains(2.5)}")
"#,
        "true
1
true",
    );
}

#[test]
fn test_set_int_remove_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set({1, 2})
    let removed = s.remove(1)
    println(f"{removed}")
    println(f"{s.length()}")
"#,
        "true
1",
    );
}

#[test]
fn test_set_i128_distinguishes_elements_by_the_whole_value() {
    // `i128::MAX` and `-1` fill their low eight bytes with the same ones and
    // differ only above bit 63, so a set that compared a value word alone would
    // call them equal and hold one of them for both.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>()
    s.add(big)
    println(f"{s.contains(big)} {s.contains(twin)}")
    s.add(twin)
    println(f"{s.length()} {s.contains(twin)}")
"#,
        "true false
2 true",
    );
}

#[test]
fn test_set_i128_removes_only_the_element_asked_for() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>()
    s.add(big)
    s.add(twin)
    println(f"{s.remove(twin)} {s.length()}")
    println(f"{s.contains(big)} {s.contains(twin)}")
"#,
        "true 1
true false",
    );
}

#[test]
fn test_set_u128_distinguishes_elements_above_the_low_word() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let all_ones u128 = ~(0 as u128)
    let low_ones u128 = 18446744073709551615
    var s = Set<u128>()
    s.add(all_ones)
    println(f"{s.contains(all_ones)} {s.contains(low_ones)}")
    s.add(low_ones)
    println(f"{s.length()}")
"#,
        "true false
2",
    );
}

#[test]
fn test_set_i128_deduplicates_the_same_element() {
    // Passes today, because the same element has the same low word either
    // way. It guards against a widening fix over-correcting into storing a
    // duplicate, and proves nothing about the upper half on its own.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var s = Set<i128>()
    s.add(big)
    s.add(big)
    println(f"{s.length()}")
"#,
        "1",
    );
}

#[test]
fn test_set_i128_literal_keeps_the_whole_element() {
    // A set built from a literal populates itself at a second emit site, which
    // hands each element over without passing through the method the later
    // lookups call. Both have to spell a sixteen-byte element the same way.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>({big, twin})
    println(f"{s.length()}")
    println(f"{s.contains(big)} {s.contains(twin)}")
"#,
        "2
true true",
    );
}

#[test]
fn test_set_i128_in_operator_reads_the_whole_element() {
    // `in` lowers at a seam of its own, so a wide element has to be spelled
    // there the way the store spelled it.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>({})
    s.add(big)
    println(f"{big in s} {twin in s}")
"#,
        "true false",
    );
}

#[test]
fn test_set_i128_lookup_widens_a_narrow_negative() {
    // A lookup spelled at `int` has to reach the set as the sixteen-byte
    // element the store wrote, sign bit and all: zero-filling the upper half of
    // a negative would search for a value nothing put there.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let narrow int = -5
    var s = Set<i128>({})
    s.add(-5)
    println(f"{s.contains(narrow)} {narrow in s}")
    println(f"{s.remove(narrow)} {s.length()}")
"#,
        "true true
true 0",
    );
}

#[test]
fn test_set_i128_stores_a_narrow_negative_at_the_slot_width() {
    // The other direction: a narrow negative going in must be widened the same
    // way, or the wide value it equals will not find it.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let narrow int = -7
    let wide i128 = -7
    var s = Set<i128>({})
    s.add(narrow)
    println(f"{s.contains(wide)} {s.length()}")
"#,
        "true 1",
    );
}

#[test]
fn test_set_of_inline_vectors_agrees_between_the_literal_and_the_method() {
    // A vector element is laid out in the set's buffer rather than referenced,
    // so its operand is already the address the set copies from. Both seams
    // that hand the set an element have to know that: spilling the operand at
    // one of them would store the address instead of the components, and two
    // vectors with the same components would then count as two elements at one
    // seam and one at the other.
    let source = "
use system.gpu.vector
use system.collections.set

fn main()
    let a = Vec2<f32>(1.0, 2.0)
    let same = Vec2<f32>(1.0, 2.0)
    let b = Vec2<f32>(3.0, 4.0)
    var built = Set<Vec2<f32>>({})
    built.add(a)
    built.add(same)
    built.add(b)
    let literal = Set<Vec2<f32>>({a, same, b})
    println(f'{built.length()} {literal.length()} {built.contains(same)} {literal.contains(b)}')
";
    assert_runs_with_output(source, "2 2 true true");
}

#[test]
fn test_set_f32_finds_the_element_it_stored() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<f32>()
    s.add(1.5)
    s.add(2.5)
    s.add(1.5)
    println(f"{s.length()} {s.contains(2.5)} {s.contains(3.5)}")
    println(f"{s.remove(1.5)} {s.length()}")
"#,
        "2 true false
true 1",
    );
}
