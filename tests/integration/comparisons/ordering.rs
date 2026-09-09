// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Ordering operators (`<`, `<=`, `>`, `>=`) answer from the operands' content,
//! and are refused on types that carry no ordering.

use super::utils::*;

#[test]
fn test_string_ordering_compares_content_not_addresses() {
    // `upper` is built at runtime, so it is allocated after `lower` and would
    // compare greater under an address comparison. 'H' (72) precedes 'h' (104).
    assert_runs_with_output(
        r#"
fn main()
    let lower = "hello"
    let upper = lower.to_upper()
    let lt = upper < lower
    let gt = upper > lower
    let le = upper <= lower
    let ge = upper >= lower
    println(f"lt={lt} gt={gt} le={le} ge={ge}")
"#,
        "lt=true gt=false le=true ge=false",
    );
}

#[test]
fn test_string_ordering_on_equal_content_is_not_strict() {
    assert_runs_with_output(
        r#"
fn main()
    let a = "MIRI".to_lower()
    let b = "miri"
    let lt = a < b
    let gt = a > b
    let le = a <= b
    let ge = a >= b
    println(f"lt={lt} gt={gt} le={le} ge={ge}")
"#,
        "lt=false gt=false le=true ge=true",
    );
}

#[test]
fn test_string_ordering_is_lexicographic_over_a_shared_prefix() {
    assert_runs_with_output(
        r#"
fn main()
    let long = "cart"
    let short = long.substring(0, 3)
    let lt = short < long
    let gt = short > long
    println(f"lt={lt} gt={gt}")
"#,
        "lt=true gt=false",
    );
}

#[test]
fn test_sorting_runtime_built_strings_yields_alphabetical_order() {
    // Every element is built at runtime, so the answer cannot come from the
    // order the string pool happens to lay literals out in.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())
    words.push("FIG".to_lower())
    words.push("DATE".to_lower())
    var i = 1
    while i < words.length()
        let key = words[i]
        var j = i
        while j > 0 and key < words[j - 1]
            words.set(j, words[j - 1])
            j -= 1
        words.set(j, key)
        i += 1
    println(",".join(words))
"#,
        "apple,date,fig,pear",
    );
}

#[test]
fn test_struct_without_ordering_is_rejected() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let a = P(1)
    let b = P(2)
    println(f"{a < b}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_ordering_rejection_names_a_member_that_does_compare() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let a = P(1)
    let b = P(2)
    println(f"{a < b}")
"#,
        "a.v < b.v",
    );
}

#[test]
fn test_class_without_ordering_is_rejected() {
    assert_compiler_error(
        r#"
class Box
    v int

    fn init(v int)
        self.v = v

fn main()
    let a = Box(1)
    let b = Box(2)
    println(f"{a >= b}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_booleans_order_by_value() {
    // `bool` is kept orderable: `false` sorts before `true`, and the comparison
    // reads the value rather than an address, so it has no way to be wrong.
    assert_runs_with_output(
        r#"
fn main()
    let f = false
    let t = true
    println(f"{f < t} {t < f} {f <= f}")
"#,
        "true false true",
    );
}

#[test]
fn test_lists_have_no_ordering() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    let a = List<int>()
    let b = List<int>()
    println(f"{a < b}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_equality_still_works_on_a_type_without_ordering() {
    // The refusal is confined to the ordering operators: `==` on a struct is
    // structural and stays available.
    assert_runs_with_output(
        r#"
struct P
    v int

fn main()
    let a = P(1)
    let b = P(1)
    let c = P(2)
    println(f"{a == b} {a == c}")
"#,
        "true false",
    );
}

#[test]
fn test_class_implementing_comparable_orders_by_its_own_rule() {
    assert_runs_with_output(
        r#"
use system.ops

class Weight implements Comparable
    grams int

    fn init(grams int)
        self.grams = grams

    public fn compare(other Weight) int
        return self.grams - other.grams

fn main()
    let light = Weight(10)
    let heavy = Weight(20)
    let lt = light < heavy
    let gt = light > heavy
    let ge = light >= heavy
    println(f"lt={lt} gt={gt} ge={ge}")
"#,
        "lt=true gt=false ge=false",
    );
}

#[test]
fn test_integer_and_float_ordering_are_unaffected() {
    assert_runs_with_output(
        r#"
fn main()
    let i = 3
    let j = 7
    let x = 2.5
    let y = 1.5
    println(f"{i < j} {x > y} {i >= j} {y <= x}")
"#,
        "true true false true",
    );
}

#[test]
fn test_a_string_parameter_guard_admits_a_value_its_content_orders_after() {
    assert_runs_with_output(
        r#"
fn after_m(word String > "m") String
    return word

fn main()
    let z = "ZEBRA".to_lower()
    println(after_m(z))
"#,
        "zebra",
    );
}

#[test]
fn test_a_string_parameter_guard_refuses_a_value_its_content_orders_before() {
    // Built at runtime, so it is allocated after the literal the guard names
    // and would pass an address comparison. 'apple' sorts before 'm'.
    assert_runtime_crash(
        r#"
fn after_m(word String > "m") String
    return word

fn main()
    let a = "APPLE".to_lower()
    println(after_m(a))
"#,
    );
}

#[test]
fn test_a_string_parameter_guard_compares_inequality_by_content() {
    assert_runtime_crash(
        r#"
fn not_stop(word String != "stop") String
    return word

fn main()
    let s = "STOP".to_lower()
    println(not_stop(s))
"#,
    );
}
