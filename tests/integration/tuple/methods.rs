// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_length() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.length()}")
"#,
        "3",
    );
}

#[test]
fn test_tuple_length_single() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (42,)
println(f"{t.length()}")
"#,
        "1",
    );
}

#[test]
fn test_tuple_length_pair() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (1, 2)
println(f"{t.length()}")
"#,
        "2",
    );
}

#[test]
fn test_tuple_element_at() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.element_at(0)}")
println(f"{t.element_at(1)}")
println(f"{t.element_at(2)}")
"#,
        "10\n20\n30",
    );
}

#[test]
fn test_tuple_is_empty() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (1, 2, 3)
println(f"{t.is_empty()}")
"#,
        "false",
    );
}

#[test]
fn test_tuple_first() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.first() ?? -1}")
"#,
        "10",
    );
}

#[test]
fn test_tuple_last() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.last() ?? -1}")
"#,
        "30",
    );
}

#[test]
fn test_tuple_first_single() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (42,)
println(f"{t.first() ?? -1}")
println(f"{t.last() ?? -1}")
"#,
        "42\n42",
    );
}

#[test]
fn test_tuple_contains() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.contains(20)}")
println(f"{t.contains(99)}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_tuple_index_of() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.index_of(20) ?? -1}")
match t.index_of(99)
    Some(idx)
        println(f"{idx}")
    None
        println("not_found")
"#,
        "1\nnot_found",
    );
}

#[test]
fn test_tuple_index_of_first_element() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.index_of(10) ?? -1}")
"#,
        "0",
    );
}

#[test]
fn test_tuple_index_of_last_element() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.index_of(30) ?? -1}")
"#,
        "2",
    );
}

#[test]
fn test_tuple_string_queryable_methods() {
    assert_runs_with_output(
        r#"
use system.collections.tuple

let t = ("hello", "world", "foo")
let first = t.first() ?? "none"
let last = t.last() ?? "none"
println(first)
println(last)
let c_yes = t.contains("world")
let c_no = t.contains("zzz")
println(f"{c_yes}")
println(f"{c_no}")
let i_first = t.index_of("hello") ?? -1
let i_last = t.index_of("foo") ?? -1
let i_missing = match t.index_of("zzz")
    Some(idx)
        idx
    None
        -1
println(f"{i_first}")
println(f"{i_last}")
println(f"{i_missing}")
println(f"{t.is_empty()}")
"#,
        "hello\nfoo\ntrue\nfalse\n0\n2\n-1\nfalse",
    );
}

/// Two tuples of one element type and different lengths reach their own
/// methods: each answers for its own length.
#[test]
fn test_tuples_of_one_element_type_and_two_lengths_answer_for_their_own() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

fn main()
    let pair = (1, 2)
    let triple = (1, 2, 3)
    println(f"{pair.length()} {triple.length()}")
    println(f"{pair.contains(2)} {pair.contains(3)} {triple.contains(3)}")
    println(f"{pair.last() ?? -1} {triple.last() ?? -1} {triple.index_of(3) ?? -1}")
"#,
        "2 3\ntrue false true\n2 3 2",
    );
}

/// Elements narrower and wider than a word are read at their own stride.
#[test]
fn test_tuple_methods_read_elements_of_every_width() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

fn main()
    let small = (1.5 as f32, 2.5 as f32, 3.5 as f32)
    let at_small = small.index_of(3.5 as f32) ?? -1
    println(f"{small.length()} {small.contains(2.5 as f32)} {at_small}")
    let flags = (false, false, true, false)
    let at_flag = flags.index_of(true) ?? -1
    let last_flag = flags.last() ?? true
    println(f"{flags.length()} {at_flag} {last_flag}")
    let big = (9223372036854775807 as i128) * (4 as i128)
    let wide = (big, 2 as i128, 3 as i128)
    let at_wide = wide.index_of(2 as i128) ?? -1
    println(f"{wide.length()} {wide.contains(3 as i128)} {at_wide}")
    let head = wide.first() ?? (0 as i128)
    println(f"{head == big} {wide.contains(big)} {wide.contains(big - (1 as i128))}")
    var total = 0.0 as f32
    for x in small
        total += x
    println(f"{total}")
"#,
        "3 true 2\n4 2 false\n3 true 1\ntrue true false\n7.5",
    );
}

#[test]
fn test_tuple_methods_on_a_tuple_of_tuples() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

fn main()
    let t = ((1, 2), (3, 4))
    println(f"{t.length()} {t.contains((3, 4))} {t.contains((4, 3))} {t.index_of((3, 4)) ?? -1}")
    var sum = 0
    for pair in t
        sum += pair.0 * pair.1
    println(f"{sum}")
"#,
        "2 true false 1\n14",
    );
}

/// A tuple of strings is a counted allocation its methods borrow: every method
/// leaves each string with the one owner it had.
#[test]
fn test_every_tuple_method_on_a_string_tuple_releases_what_it_reads() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.tuple

fn describe(t (String, String, String)) String
    let has_b = t.contains("b")
    let at_c = t.index_of("c") ?? -1
    return f"{t.length()} {has_b} {at_c} {t.is_empty()}"

fn main()
    let t = (f"a{1}", "b", f"c{3}")
    println(describe(t))
    let head = t.first() ?? "-"
    let tail = t.last() ?? "-"
    println(f"{head} {tail} {t.element_at(1)}")
    var joined = ""
    for s in t
        joined = joined + s
    println(joined)
"#,
        "3 true -1 false\na1 c3 b\na1bc3",
    );
}

/// Every shape of tuple is its own instance of the tuple class, at its element
/// type and length. A length is only ever one the program writes, so however
/// many shapes a program holds, a generic function reaching one more is not
/// taken for an instantiation growing without bound.
#[test]
fn test_a_program_with_hundreds_of_tuple_shapes_compiles() {
    let element_types = [
        "int", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "f32", "float",
    ];
    let mut body = String::new();
    let mut expected = 0;
    for (kind, element_type) in element_types.iter().enumerate() {
        for length in 1..=26 {
            let elements = vec![format!("(1 as {element_type})"); length].join(", ");
            let trailing = if length == 1 { "," } else { "" };
            body.push_str(&format!(
                "    let t{kind}_{length} = ({elements}{trailing})\n    total += t{kind}_{length}.length()\n"
            ));
            expected += length;
        }
    }
    let program = format!(
        "use system.io\nuse system.collections.tuple\n\nfn pair_length<T>(t (T, T)) int\n    return t.length()\n\nfn main()\n    var total = 0\n{body}    total += pair_length((true, false))\n    println(f\"{{total}}\")\n"
    );
    assert_runs_with_output(&program, &(expected + 2).to_string());
}
