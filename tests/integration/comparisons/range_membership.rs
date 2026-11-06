// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `x in r` on a range of numbers holds exactly when `for v in r` visits `x`:
//! at or above the start, and below the end, or at it when the end is
//! included.

use super::utils::*;
use crate::utils::miri_check;

/// Assert that checking `code` reports exactly one diagnostic, an error,
/// whose report carries every one of `fragments`.
fn assert_single_diagnostic(code: &str, fragments: &[&str]) {
    let output = miri_check(code).output();
    let reported = output.matches("error[").count() + output.matches("warning[").count();
    assert_eq!(reported, 1, "expected exactly one diagnostic:\n{output}");
    for fragment in fragments {
        assert!(
            output.contains(fragment),
            "expected `{fragment}` in:\n{output}"
        );
    }
}

#[test]
fn an_exclusive_range_holds_its_start_but_not_its_end() {
    assert_runs_with_output(
        r#"
fn main()
    var out = ""
    for i in -1..12
        if i in 0..10
            out = out + "e"
        else
            out = out + "."
    println(out)
"#,
        ".eeeeeeeeee..",
    );
}

#[test]
fn an_inclusive_range_holds_its_end() {
    assert_runs_with_output(
        r#"
fn main()
    var out = ""
    for i in -1..12
        if i in 0..=10
            out = out + "i"
        else
            out = out + "."
    println(out)
"#,
        ".iiiiiiiiiii.",
    );
}

#[test]
fn a_range_value_answers_by_its_own_bounds() {
    assert_runs_with_output(
        r#"
fn main()
    let r = 3..5
    let a i8 = 4
    println(f"{a in r} {5 in r} {2 in r} {4 in 5..3}")
"#,
        "true false false false",
    );
}

#[test]
fn a_generic_body_tests_membership_at_each_instantiation() {
    assert_runs_with_output(
        r#"
fn within<T>(x T, r Range<T>) bool
    return x in r

fn main()
    println(f"{within(4, 3..5)} {within(1.5, 1.0..2.0)} {within(2.0, 1.0..2.0)}")
"#,
        "true true false",
    );
}

#[test]
fn the_value_is_read_before_the_range_is_built() {
    assert_runs_with_output(
        r#"
fn grow(n out int) int
    n = 99
    return 10

fn main()
    var n = 5
    println(f"{n in 0..grow(n)} {n}")
"#,
        "true 99",
    );
}

#[test]
fn a_parameter_guard_on_a_range_admits_values_inside_it() {
    assert_runs_with_output(
        r#"
fn pick(x int in 0..10) int
    return x * 2

fn main()
    println(f"{pick(0)} {pick(9)}")
"#,
        "0 18",
    );
}

#[test]
fn a_parameter_guard_on_a_range_refuses_its_excluded_end() {
    assert_runtime_error(
        r#"
fn pick(x int in 0..10) int
    return x * 2

fn main()
    println(f"{pick(10)}")
"#,
        "pick: x must be in 0..10",
    );
}

#[test]
fn a_written_range_evaluates_the_value_and_each_bound_once_left_to_right() {
    assert_runs_with_output(
        r#"
fn note(log out String, tag String, v int) int
    log = log + tag
    return v

fn main()
    var log = ""
    let a = note(log, "x", 3) in note(log, "a", 3)..note(log, "b", 5)
    let b = note(log, "x", 5) in note(log, "a", 3)..=note(log, "b", 5)
    let c = note(log, "x", 5) in note(log, "a", 3)..note(log, "b", 5)
    println(f"{a} {b} {c} {log}")
"#,
        "true true false xabxabxab",
    );
}

#[test]
fn a_written_range_holds_an_i8_a_float_and_a_negative_value_by_its_bounds() {
    assert_runs_with_output(
        r#"
fn main()
    let w i8 = 4
    let lo = 2
    let hi = 4
    println(f"{w in lo..hi} {w in lo..=hi} {1.5 in 1.0..2.0} {-1 in -1..0}")
"#,
        "false true true true",
    );
}

#[test]
fn a_written_range_tested_in_a_cpu_forall_holds_by_its_bounds() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    var dst = Array<int, 8>()
    forall i in 0..8
        if i in 2..5: dst[i] = 1
        if i in 6..=7: dst[i] = 2
    var out = ""
    for i in 0..8
        out = out + f"{dst[i]}"
    println(out)
"#,
        "00111022",
    );
}

#[test]
fn a_float_is_in_a_range_of_f32_exactly_when_the_comparisons_say_so() {
    assert_runs_with_output(
        r#"
fn main()
    let lo f32 = -0.5
    let hi f32 = 0.5
    let inside float = 0.25
    let past float = 0.5000000001
    println(f"{inside in lo..=hi} {past in lo..=hi} {past >= lo and past <= hi}")
"#,
        "true false false",
    );
}

#[test]
fn a_range_value_whose_end_was_written_tests_against_the_new_end() {
    assert_runs_with_output(
        r#"
fn main()
    var r = 0..3
    let before = 4 in r
    r.end = 5
    println(f"{before} {4 in r} {5 in r}")
"#,
        "false true false",
    );
}

#[test]
fn membership_refuses_a_value_the_comparisons_refuse() {
    assert_compiler_error(
        r#"
fn main()
    let s = "b"
    println(f"{s in 1..3}")
"#,
        "cannot check membership of String in range of int",
    );
}

#[test]
fn a_float_local_is_tested_against_float_literal_bounds() {
    assert_runs_with_output(
        r#"
fn main()
    var out = ""
    for i in 0..8
        let x = (i as float) * 0.25 - 1.0
        if x in -0.5..=0.5
            out = out + "1"
        else
            out = out + "0"
    println(out)
"#,
        "00111110",
    );
}

#[test]
fn a_string_is_in_a_written_range_of_strings_by_content() {
    assert_heap_guard_output(
        r#"
fn main()
    let b = "b"
    let bz = "bz"
    let e = "e"
    println(f"{b in 'a'..'e'} {bz in 'a'..'b'} {e in 'a'..='e'} {e in 'a'..'e'}")
"#,
        "true false true false",
    );
}

#[test]
fn a_string_range_value_answers_by_its_bounds_and_its_inclusive_flag() {
    assert_heap_guard_output(
        r#"
fn main()
    let sr = "a".."e"
    var flip = "a".."e"
    let before = "e" in flip
    flip.inclusive = true
    println(f"{'b' in sr} {'e' in sr} {'0' in sr} {before} {'e' in flip}")
"#,
        "true false false false true",
    );
}

#[test]
fn a_string_built_for_the_test_is_released_after_it() {
    assert_heap_guard_output(
        r#"
fn main()
    var out = ""
    for i in 0..3
        if f"b{i}" in "a".."b1"
            out = out + "y"
        else
            out = out + "n"
    let lo = "a".to_upper()
    let c = f"c{out.length()}" in lo.."d"
    println(f"{out} {c}")
"#,
        "ynn true",
    );
}

#[test]
fn a_bool_is_in_a_range_of_bools_by_its_order() {
    assert_heap_guard_output(
        r#"
fn main()
    let t = true
    let f = false
    println(f"{t in f..=t} {t in f..t} {f in f..t} {f in t..=t}")
"#,
        "true false true false",
    );
}

#[test]
fn a_comparable_class_is_tested_through_its_compare_alone() {
    assert_heap_guard_output(
        r#"
class V implements Comparable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other V) int
        return self.n - other.n

fn main()
    let r = V(1)..V(3)
    println(f"{V(2) in V(1)..V(3)} {V(3) in V(1)..V(3)} {V(3) in V(1)..=V(3)} {V(2) in r} {V(0) in r}")
"#,
        "true false true true false",
    );
}

#[test]
fn a_class_range_compares_the_start_first_and_skips_the_end_once_it_fails() {
    assert_heap_guard_output(
        r#"
class V implements Comparable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other V) int
        print(f"[{self.n}:{other.n}]")
        return self.n - other.n

fn main()
    let lo = V(1)
    let hi = V(3)
    let inside = V(2) in lo..hi
    println("")
    let below = V(0) in lo..hi
    println("")
    let r = lo..=hi
    let past = V(4) in r
    println("")
    println(f"{inside} {below} {past}")
"#,
        "[1:2][2:3]\n[1:0]\n[1:4][4:3]\ntrue false false",
    );
}

#[test]
fn a_string_range_evaluates_the_value_and_each_bound_once_left_to_right() {
    assert_heap_guard_output(
        r#"
fn note(log out String, tag String, v String) String
    log = log + tag
    return v

fn main()
    var log = ""
    let a = note(log, "x", "b") in note(log, "a", "a")..note(log, "b", "c")
    let b = note(log, "x", "0") in note(log, "a", "a")..=note(log, "b", "c")
    println(f"{a} {b} {log}")
"#,
        "true false xabxab",
    );
}

#[test]
fn a_comparable_bound_generic_body_tests_membership_at_int_and_string() {
    assert_heap_guard_output(
        r#"
fn within<T implements Comparable>(x T, a T, b T) bool
    return x in a..b

fn main()
    println(f"{within(2, 1, 3)} {within('b', 'a', 'c')} {within('d', 'a', 'c')}")
"#,
        "true true false",
    );
}

#[test]
fn an_unbounded_generic_body_tests_membership_of_strings() {
    assert_heap_guard_output(
        r#"
fn within<T>(x T, a T, b T) bool
    return x in a..b

fn main()
    println(f"{within('b', 'a', 'c')} {within(5, 1, 3)}")
"#,
        "true false",
    );
}

#[test]
fn an_unbounded_generic_membership_at_a_struct_is_refused_at_the_call() {
    assert_compiler_error(
        r#"
struct P
    a int

fn within<T>(x T, a T, b T) bool
    return x in a..b

fn main()
    println(f"{within(P(a: 1), P(a: 0), P(a: 3))}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn membership_of_an_optional_in_a_range_names_the_missing_ordering() {
    assert_single_diagnostic(
        r#"
fn main()
    var x int? = 2
    x = 3
    println(f"{x in 1..3}")
"#,
        &[
            "[MER_TYP_075]",
            "Type 'int?' has no ordering: testing it against a range's bounds requires the \
             Comparable trait",
            "unwrap the optional before testing it",
        ],
    );
}

#[test]
fn membership_in_a_range_of_structs_names_the_missing_ordering() {
    assert_compiler_error(
        r#"
struct P
    a int

fn main()
    let p = P(a: 1)
    println(f"{p in P(a: 0)..P(a: 3)}")
"#,
        "`x in r` compares `r.start <= x` and `x < r.end`",
    );
}

#[test]
fn membership_of_a_string_in_a_range_of_ints_is_a_type_mismatch() {
    let source = r#"
fn main()
    let s = "x"
    println(f"{s in 1..4}")
"#;
    assert_compiler_error(source, "[MER_TYP_002]");
    assert_compiler_error(
        source,
        "a value of type `int` to test against a `Range<int>`",
    );
}

#[test]
fn an_argument_read_before_a_range_test_that_writes_it_keeps_its_value() {
    assert_heap_guard_output(
        r#"
class Cell
    var n int
    fn init(n int)
        self.n = n

class V implements Comparable
    var n int
    var cell Cell
    fn init(n int, cell Cell)
        self.n = n
        self.cell = cell
    public fn compare(other V) int
        self.cell.n = 99
        return self.n - other.n

fn pair(a int, t bool) String
    return f"{a} {t}"

fn main()
    let cell = Cell(1)
    let lo = V(1, cell)
    let hi = V(3, cell)
    let v = V(2, cell)
    println(pair(cell.n, v in lo..hi))
"#,
        "1 true",
    );
}

#[test]
fn a_range_of_a_base_class_is_refused_for_a_value_its_bounds_cannot_compare_with() {
    assert_compiler_error(
        r#"
class V implements Comparable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other V) int
        return self.n - other.n

class W extends V
    fn init(n int)
        super.init(n)

fn main()
    let v = V(2)
    println(f"{v in W(1)..W(3)}")
"#,
        "[MER_TYP_002]",
    );
}

#[test]
fn a_struct_tested_against_a_range_of_ints_is_a_type_mismatch() {
    assert_single_diagnostic(
        r#"
struct P
    a int

fn main()
    let p = P(a: 1)
    println(f"{p in 1..3}")
"#,
        &[
            "[MER_TYP_002]",
            "cannot check membership of P in range of int",
        ],
    );
}

#[test]
fn a_bound_already_refused_reports_nothing_more() {
    assert_single_diagnostic(
        r#"
fn main()
    println(f"{1.0 in 0.0 / 0.0..2.0}")
"#,
        &["Division by zero"],
    );
}

#[test]
fn a_range_tested_against_a_range_of_ranges_names_a_member_that_orders() {
    assert_single_diagnostic(
        r#"
fn main()
    let r1 = 1..2
    let r2 = 0..1
    let r3 = 2..3
    println(f"{r1 in r2..r3}")
"#,
        &[
            "[MER_TYP_075]",
            "Type 'Range<int>' has no ordering: testing it against a range's bounds",
            "test a member that orders, such as `start`",
        ],
    );
}

#[test]
fn wide_integers_are_in_a_range_by_their_full_value() {
    assert_runs_with_output(
        r#"
fn main()
    let lo i128 = -100000000000000000000000
    let hi i128 = 100000000000000000000000
    let near i128 = 99999999999999999999999
    let top u128 = 300000000000000000000000000000000000000
    let floor u128 = 0
    let a = hi in lo..=hi
    let b = hi in lo..hi
    let c = near in lo..hi
    let d = top in floor..top
    let e = top in floor..=top
    println(f"{a} {b} {c} {d} {e}")
"#,
        "true false true false true",
    );
}

#[test]
fn nan_is_in_no_range_and_no_range_with_a_nan_bound_holds_a_value() {
    assert_runs_with_output(
        r#"
fn half(x float) float
    return x / 2.0

fn main()
    let zero = half(0.0)
    let n = zero / zero
    println(f"{n in 0.0..1.0} {1.0 in n..2.0} {1.0 in 0.0..n} {1.0 in 0.0..=n}")
"#,
        "false false false false",
    );
}

#[test]
fn a_string_range_with_equal_or_crossed_bounds_holds_only_an_included_end() {
    assert_heap_guard_output(
        r#"
fn main()
    let b = "b"
    let e = ""
    println(f"{b in 'b'..'a'} {b in 'b'..'b'} {b in 'b'..='b'} {e in ''..=''}")
"#,
        "false false true true",
    );
}

#[test]
fn a_string_range_orders_by_code_point() {
    assert_heap_guard_output(
        r#"
fn main()
    let accented = "é"
    println(f"{accented in 'a'..'z'} {accented in 'a'..='é'}")
"#,
        "false true",
    );
}

#[test]
fn a_u8_is_tested_against_literal_bounds_by_its_value() {
    assert_runs_with_output(
        r#"
fn main()
    let b u8 = 200
    println(f"{b in 100..255} {b in 0..200} {b in 200..=200} {b in 201..=255}")
"#,
        "true false true false",
    );
}

#[test]
fn a_generic_range_value_body_tests_ints_strings_and_classes() {
    assert_heap_guard_output(
        r#"
class V implements Comparable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other V) int
        return self.n - other.n

fn within<T>(x T, r Range<T>) bool
    return x in r

fn main()
    let s = "b"
    println(f"{within(2, 1..3)} {within(s, 'a'..'b')} {within(V(2), V(1)..=V(2))}")
"#,
        "true false true",
    );
}

#[test]
fn a_generic_range_value_body_at_a_struct_is_refused_at_the_call() {
    assert_compiler_error(
        r#"
struct P
    a int

fn within<T>(x T, r Range<T>) bool
    return x in r

fn main()
    println(f"{within(P(a: 1), P(a: 0)..P(a: 3))}")
"#,
        "[MER_TYP_075]",
    );
}

#[test]
fn membership_in_a_range_of_tuples_names_the_missing_ordering() {
    assert_compiler_error(
        r#"
fn main()
    let t = (1, 2)
    println(f"{t in (0, 0)..(3, 3)}")
"#,
        "[MER_TYP_075]",
    );
}

#[test]
fn membership_in_a_range_of_enum_values_names_the_missing_ordering() {
    assert_compiler_error(
        r#"
enum Size
    Small
    Large

fn main()
    let s = Size.Small
    println(f"{s in Size.Small..Size.Large}")
"#,
        "[MER_TYP_075]",
    );
}
