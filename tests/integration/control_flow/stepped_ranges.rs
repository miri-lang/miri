// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `for x in a..b` steps through a range of any `Steppable` type: the
//! integers by adding one, a one-character string by Unicode code point, and a
//! class through its own `successor`. A range whose type cannot be stepped is
//! refused, and a bound that cannot be stepped stops the loop before its first
//! pass.

use super::utils::*;
use crate::utils::{miri_check, miri_run};

/// The sentence a range bound that cannot be stepped stops the program with;
/// its code, MER_RT_019, is pinned by the trap-code gates.
const UNSTEPPABLE_BOUND: &str = "a range bound cannot be stepped";

/// The sentence a value with no successor stops the program with.
const NO_SUCCESSOR: &str = "has no successor";

/// Assert that running `code` stops because a bound cannot be stepped, and
/// never prints `body`.
fn assert_stopped_before_first_pass(code: &str, body: &str) {
    let result = miri_run(code);
    let output = result.output();
    assert!(!result.success, "expected the run to stop:\n{output}");
    assert!(
        output.contains(UNSTEPPABLE_BOUND),
        "expected `{UNSTEPPABLE_BOUND}`:\n{output}"
    );
    assert!(
        !result.stdout.contains(body),
        "the body ran before the trap:\n{output}"
    );
}

const STEPPABLE_CLASS: &str = r#"
class V implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other V) int
        return self.n - other.n
    public fn successor() V
        return V(self.n + 1)
"#;

#[test]
fn a_string_range_steps_by_code_point_and_leaves_its_end_out() {
    assert_heap_guard_output(
        r#"
fn main()
    for c in 'a'..'e'
        print(c)
    println("")
    for c in "a"..="e"
        print(c)
    println("")
"#,
        "abcd\nabcde",
    );
}

#[test]
fn a_string_range_backwards_runs_no_pass_and_one_at_its_end() {
    assert_heap_guard_output(
        r#"
fn main()
    var passes = 0
    for c in "e".."a"
        passes = passes + 1
    for c in "a"..="a"
        passes = passes + 10
    println(f"{passes}")
"#,
        "10",
    );
}

#[test]
fn a_string_range_steps_multi_byte_characters() {
    assert_heap_guard_output(
        r#"
fn main()
    for c in "é"..="ë"
        print(c)
    println("")
    for c in "😀"..="😂"
        print(c)
    println("")
"#,
        "éêë\n😀😁😂",
    );
}

#[test]
fn a_string_range_skips_the_surrogate_gap_and_reaches_the_last_code_point() {
    assert_heap_guard_output(
        r#"
fn main()
    let below = String.from_code_point(0xD7FF) ?? ""
    let above = String.from_code_point(0xE000) ?? ""
    var passes = 0
    for c in below..=above
        passes = passes + 1
    let last = String.from_code_point(0x10FFFF) ?? ""
    let before_last = String.from_code_point(0x10FFFE) ?? ""
    for c in before_last..=last
        passes = passes + 10
    println(f"{passes}")
"#,
        "22",
    );
}

#[test]
fn a_string_range_value_is_stepped() {
    assert_heap_guard_output(
        r#"
fn main()
    let r = "a".."e"
    for c in r
        print(c)
    println("")
    var q = "x".."z"
    q.inclusive = true
    for c in q
        print(c)
    println("")
"#,
        "abcd\nxyz",
    );
}

#[test]
fn a_string_loop_breaks_continues_returns_and_keeps_its_values() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn first_after(start String) String
    for c in start..="z"
        if c > "m"
            return c
    return ""

fn main()
    var kept = List<String>()
    for c in "a"..="z"
        if c == "c"
            continue
        if c == "f"
            break
        kept.push(c)
    println(f"{kept.length()} {kept[0]}{kept[1]}{kept[2]}{kept[3]} {first_after('k')}")
"#,
        "4 abde n",
    );
}

#[test]
fn a_literal_string_bound_of_other_than_one_character_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    for c in "a".."ee"
        print(c)
"#,
        "[MER_TYP_086]",
    );
    assert_compiler_error(
        r#"
fn main()
    for c in "".."b"
        print(c)
"#,
        "[MER_TYP_086]",
    );
}

#[test]
fn a_computed_start_of_two_characters_stops_before_the_first_pass() {
    assert_stopped_before_first_pass(
        r#"
fn main()
    var lo = "a"
    lo = lo + "b"
    for c in lo.."e"
        println("pass")
"#,
        "pass",
    );
}

#[test]
fn a_computed_end_of_two_characters_stops_before_the_first_pass() {
    assert_stopped_before_first_pass(
        r#"
fn main()
    var hi = "e"
    hi = hi + "e"
    for c in "a"..hi
        println("pass")
"#,
        "pass",
    );
}

/// The checker's rule for a written bound and `String.steppable()` at run time
/// agree on every literal, including one whose two code points render as a
/// single character: a literal the checker refuses stops the program when the
/// same text reaches the loop computed, and one it accepts runs.
#[test]
fn the_literal_rule_and_steppable_agree() {
    let table = [
        ("\"\"", false),
        ("\"a\"", true),
        ("\"é\"", true),
        ("\"😀\"", true),
        ("\"ab\"", false),
        ("\"e\u{301}\"", false),
    ];
    for (literal, steppable) in table {
        let source = format!("fn main()\n    for c in {literal}..=\"z\"\n        print(c)\n");
        let checked = miri_check(&source);
        assert_eq!(
            checked.success,
            steppable,
            "the checker's verdict on {literal}:\n{}",
            checked.output()
        );
        let quoted = literal.replace('"', "'");
        let ran = miri_run(&format!(
            "fn main()\n    println(f\"{{{quoted}.steppable()}}\")\n"
        ));
        assert_eq!(
            ran.stdout.trim(),
            steppable.to_string(),
            "steppable() of {literal}:\n{}",
            ran.output()
        );
        let computed = format!(
            "fn same(s String) String\n    return s\n\nfn main()\n    for c in same({literal})..=\"😀\"\n        println(\"pass\")\n        break\n"
        );
        if steppable {
            assert_runs_with_output(&computed, "pass");
        } else {
            assert_stopped_before_first_pass(&computed, "pass");
        }
    }
}

#[test]
fn a_steppable_class_range_is_stepped_through_its_own_methods() {
    assert_heap_guard_output(
        &format!(
            r#"{STEPPABLE_CLASS}
fn main()
    for v in V(1)..V(4)
        print(f"{{v.n}}")
    println("")
    for v in V(1)..=V(4)
        print(f"{{v.n}}")
    println("")
"#
        ),
        "123\n1234",
    );
}

/// A class range runs as many passes as its `compare` allows: none when it
/// starts past or at an excluded end, one when it starts at an included end.
#[test]
fn a_class_range_at_or_past_its_end_runs_its_pass_count() {
    assert_heap_guard_output(
        &format!(
            r#"{STEPPABLE_CLASS}
fn passes(r Range<V>) int
    var count = 0
    for v in r
        count = count + 1
    return count

fn main()
    var counts = 0
    for v in V(5)..V(2)
        counts = counts + 1
    for v in V(5)..V(5)
        counts = counts + 10
    for v in V(5)..=V(5)
        counts = counts + 100
    for v in V(5)..=V(4)
        counts = counts + 1000
    let r = V(1)..V(4)
    println(f"{{counts}} {{passes(r)}} {{passes(V(1)..=V(4))}}")
"#
        ),
        "100 3 4",
    );
}

/// A closure made in a pass captures that pass's value, for a class and for
/// a string alike.
#[test]
fn a_closure_captures_the_value_of_its_pass() {
    assert_heap_guard_output(
        &format!(
            r#"{STEPPABLE_CLASS}
fn main()
    for v in V(0)..V(3)
        let tens = fn() int
            return v.n * 10
        print(f"{{tens()}} ")
    println("")
    for c in "a"..="c"
        let twice = fn() String
            return c + c
        print(f"{{twice()}} ")
    println("")
"#
        ),
        "0 10 20 \naa bb cc ",
    );
}

/// Integer, string and class loops nest inside one another and release
/// every value they step through.
#[test]
fn integer_string_and_class_loops_nest() {
    assert_heap_guard_output(
        &format!(
            r#"{STEPPABLE_CLASS}
fn main()
    var seen = 0
    for i in 0..2
        for c in "a"..="b"
            for v in V(0)..V(3)
                seen = seen + 1
    println(f"{{seen}}")
"#
        ),
        "12",
    );
}

#[test]
fn a_class_loop_breaks_and_returns_without_leaking() {
    assert_heap_guard_output(
        &format!(
            r#"{STEPPABLE_CLASS}
fn find(limit int) int
    for v in V(0)..V(100)
        if v.n == limit
            return v.n
    return -1

fn main()
    var last = 0
    for v in V(1)..=V(9)
        if v.n == 2
            continue
        if v.n == 5
            break
        last = v.n
    println(f"{{last}} {{find(7)}}")
"#
        ),
        "4 7",
    );
}

#[test]
fn a_class_bound_that_cannot_be_stepped_stops_before_the_first_pass() {
    assert_stopped_before_first_pass(
        r#"
class V implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other V) int
        return self.n - other.n
    public fn successor() V
        return V(self.n + 1)
    public fn steppable() bool
        return self.n < 10

fn main()
    for v in V(1)..V(20)
        println("pass")
"#,
        "pass",
    );
}

/// A class whose values from ten up cannot start or end a range.
const BOUNDED_CLASS: &str = r#"
class W implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other W) int
        return self.n - other.n
    public fn successor() W
        return W(self.n + 1)
    public fn steppable() bool
        return self.n < 10
"#;

/// `steppable()` says whether a value belongs to the values a range steps
/// through, so a start outside them stops the loop, and so does an end
/// outside them even when the range includes it.
#[test]
fn a_class_start_or_included_end_that_cannot_be_stepped_stops_the_loop() {
    for range in ["W(12)..W(14)", "W(1)..=W(10)"] {
        assert_stopped_before_first_pass(
            &format!(
                "{BOUNDED_CLASS}\nfn main()\n    for w in {range}\n        println(\"pass\")\n"
            ),
            "pass",
        );
    }
}

/// A range typed at a base class steps through the methods of the values it
/// holds: a subclass that counts down, orders the other way and refuses to end
/// a range is obeyed, as a written call to any of those methods is.
const BASE_AND_DESCENDING_SUB: &str = r#"
class Base implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other Base) int
        return self.n - other.n
    public fn successor() Base
        return Base(self.n + 1)
    public fn kind() String
        return "base"

class Down extends Base
    fn init(n int)
        super.init(n)
    public fn compare(other Base) int
        return other.n - self.n
    public fn successor() Down
        return Down(self.n - 1)
    public fn kind() String
        return "down"
"#;

#[test]
fn a_base_typed_range_steps_through_the_subclass_methods() {
    assert_heap_guard_output(
        &format!(
            r#"{BASE_AND_DESCENDING_SUB}
fn main()
    let a Base = Down(3)
    let b Base = Down(1)
    for x in a..=b
        print(f"{{x.n}}{{x.kind()}} ")
    println("")
"#
        ),
        "3down 2down 1down ",
    );
}

#[test]
fn a_base_typed_bound_the_subclass_refuses_stops_the_loop() {
    assert_stopped_before_first_pass(
        r#"
class Base implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other Base) int
        return self.n - other.n
    public fn successor() Base
        return Base(self.n + 1)

class Sub extends Base
    fn init(n int)
        super.init(n)
    public fn successor() Sub
        return Sub(self.n + 1)
    public fn steppable() bool
        return false

fn main()
    let a Base = Sub(1)
    let b Base = Sub(2)
    for x in a..=b
        println(f"ran {x.n}")
"#,
        "ran",
    );
}

#[test]
fn a_generic_walk_at_strings_stops_on_a_computed_bound_of_two_characters() {
    assert_stopped_before_first_pass(
        r#"
fn walk<T implements Steppable>(a T, b T)
    for x in a..b
        println("pass")

fn main()
    let tail = "c"
    walk("a", "b" + tail)
"#,
        "pass",
    );
}

#[test]
fn a_comparable_class_that_does_not_step_is_refused() {
    assert_compiler_error(
        r#"
class C implements Comparable
    var n int
    fn init(n int)
        self.n = n
    public fn compare(other C) int
        return self.n - other.n

fn main()
    for c in C(1)..C(3)
        println("pass")
"#,
        "[MER_TYP_086]",
    );
}

/// A parameter typed at the trait itself holds values of any steppable type,
/// two of which need not step through each other; the help bounds a type
/// parameter instead.
#[test]
fn a_range_of_trait_typed_values_is_refused_with_a_type_parameter_to_write() {
    let checked = miri_check(
        r#"
fn walk(a Steppable, b Steppable)
    for x in a..b
        println("pass")

fn main()
    println("unused")
"#,
    );
    let output = checked.output();
    assert!(!checked.success, "expected a refusal:\n{output}");
    assert!(
        output.contains("[MER_TYP_086]"),
        "expected MER_TYP_086:\n{output}"
    );
    assert!(
        output.contains("fn walk<T implements Steppable>"),
        "expected a type parameter to write:\n{output}"
    );
    assert!(
        !output.contains("implement Steppable on 'Steppable'"),
        "the help asks the trait to implement itself:\n{output}"
    );
}

#[test]
fn a_class_declaring_steppable_without_compare_is_refused() {
    assert_compiler_error(
        r#"
class V implements Steppable
    var n int
    fn init(n int)
        self.n = n
    public fn successor() V
        return V(self.n + 1)

fn main()
    for v in V(1)..V(3)
        println(f"{v.n}")
"#,
        "compare",
    );
}

#[test]
fn a_steppable_bound_generic_walks_ints_bytes_strings_and_classes() {
    assert_heap_guard_output(
        &format!(
            r#"{STEPPABLE_CLASS}
fn walk<T implements Steppable>(a T, b T) int
    var passes = 0
    for x in a..b
        passes = passes + 1
    return passes

fn walk_to_end<T implements Steppable>(a T, b T) int
    var passes = 0
    for x in a..=b
        passes = passes + 1
    return passes

fn main()
    let lo u8 = 250
    let hi u8 = 255
    println(f"{{walk(1, 4)}} {{walk(lo, hi)}} {{walk_to_end(lo, hi)}} {{walk('a', 'd')}} {{walk(V(2), V(7))}}")
"#
        ),
        "3 5 6 3 5",
    );
}

#[test]
fn a_steppable_bound_refuses_a_float_and_a_bool_at_the_call() {
    for call in ["walk(1.5, 2.5)", "walk(false, true)"] {
        assert_compiler_error(
            &format!(
                r#"
fn walk<T implements Steppable>(a T, b T) int
    var passes = 0
    for x in a..b
        passes = passes + 1
    return passes

fn main()
    println(f"{{{call}}}")
"#
            ),
            "does not satisfy constraint implements Steppable",
        );
    }
}

#[test]
fn an_unbounded_generic_loop_over_its_parameter_is_refused_at_the_definition() {
    let checked = miri_check(
        r#"
fn g<T>(a T, b T) int
    var passes = 0
    for x in a..b
        passes = passes + 1
    return passes

fn main()
    println("unused")
"#,
    );
    let output = checked.output();
    assert!(!checked.success, "expected a refusal:\n{output}");
    assert!(
        output.contains("[MER_TYP_086]"),
        "expected MER_TYP_086:\n{output}"
    );
    assert!(
        output.contains("fn g<T implements Steppable>"),
        "expected the bound to write:\n{output}"
    );
}

#[test]
fn an_integer_answers_successor_with_a_checked_add() {
    assert_runs_with_output(
        r#"
fn main()
    let small i8 = 3
    println(f"{3.successor()} {small.successor()} {7.steppable()}")
"#,
        "4 4 true",
    );
    assert_runtime_error(
        r#"
fn main()
    let top = 127 as i8
    println(f"{top.successor()}")
"#,
        "integer overflow",
    );
}

#[test]
fn a_string_answers_successor_by_code_point() {
    assert_heap_guard_output(
        r#"
fn main()
    println(f"{'a'.successor()} {'z'.successor()} {'é'.successor()}")
"#,
        "b { ê",
    );
    assert_runtime_error(
        r#"
fn main()
    println(f"{'ab'.successor()}")
"#,
        NO_SUCCESSOR,
    );
    assert_runtime_error(
        r#"
fn main()
    let last = String.from_code_point(0x10FFFF) ?? ""
    println(f"{last.successor()}")
"#,
        NO_SUCCESSOR,
    );
}

/// The message a long string with no successor stops the program with shows
/// its start, not the whole text.
#[test]
fn a_long_string_with_no_successor_is_shown_shortened() {
    let result =
        miri_run("fn main()\n    let long = \"abcdefghij\" * 10\n    println(long.successor())\n");
    let output = result.output();
    assert!(!result.success, "expected the run to stop:\n{output}");
    assert!(
        output.contains(NO_SUCCESSOR),
        "expected `{NO_SUCCESSOR}`:\n{output}"
    );
    assert!(
        output.contains("abcdefghijabcdefghijabcdefghijab…"),
        "expected the first 32 characters:\n{output}"
    );
    assert!(
        !output.contains("abcdefghijabcdefghijabcdefghijabc"),
        "expected the text cut at 32 characters:\n{output}"
    );
}

#[test]
fn a_bool_and_a_float_answer_no_successor() {
    assert_compiler_error(
        r#"
fn main()
    println(f"{true.successor()}")
"#,
        "Type 'bool' does not have members",
    );
    assert_compiler_error(
        r#"
fn main()
    let x = 1.5
    println(f"{x.successor()}")
"#,
        "Type 'float' does not have members",
    );
}

#[test]
fn a_float_range_is_refused_with_how_to_step_one() {
    for header in [
        "for x in 1.5..4.0",
        "for x in (1.5 as f32)..(4.0 as f32)",
        "for x in r",
    ] {
        let source =
            format!("fn main()\n    let r = 0.5..2.5\n    {header}\n        println(f\"{{x}}\")\n");
        assert_compiler_error(&source, "[MER_TYP_086]");
        assert_compiler_error(&source, "for i in 0..n");
    }
}

#[test]
fn a_bool_range_is_refused_with_the_list_to_walk_instead() {
    let source = r#"
fn main()
    for b in false..=true
        println(f"{b}")
"#;
    assert_compiler_error(source, "[MER_TYP_086]");
    assert_compiler_error(source, "[false, true]");
}

#[test]
fn a_struct_range_is_refused() {
    assert_compiler_error(
        r#"
struct P
    a int

fn main()
    for p in P(a: 1)..P(a: 3)
        println(f"{p.a}")
"#,
        "[MER_TYP_086]",
    );
}

#[test]
fn every_integer_width_still_loops_to_its_limits() {
    assert_runs_with_output(
        r#"
fn main()
    let a8 i8 = -128
    let b8 i8 = -126
    let c8 i8 = 125
    let d8 i8 = 127
    var n = 0
    for i in a8..b8
        n = n + 1
    for i in c8..=d8
        n = n + 1
    let a16 i16 = -32768
    let b16 i16 = -32767
    let c16 i16 = 32766
    let d16 i16 = 32767
    for i in a16..b16
        n = n + 1
    for i in c16..=d16
        n = n + 1
    let a32 i32 = -2147483648
    let b32 i32 = -2147483647
    let c32 i32 = 2147483646
    let d32 i32 = 2147483647
    for i in a32..b32
        n = n + 1
    for i in c32..=d32
        n = n + 1
    let c64 i64 = 9223372036854775806
    let d64 i64 = 9223372036854775807
    for i in c64..=d64
        n = n + 1
    let cu8 u8 = 254
    let du8 u8 = 255
    for i in cu8..=du8
        n = n + 1
    let cu16 u16 = 65534
    let du16 u16 = 65535
    for i in cu16..=du16
        n = n + 1
    let cu32 u32 = 4294967294
    let du32 u32 = 4294967295
    for i in cu32..=du32
        n = n + 1
    let cu64 u64 = 18446744073709551614
    let du64 u64 = 18446744073709551615
    for i in cu64..=du64
        n = n + 1
    let zu u64 = 0
    let ou u64 = 2
    for i in zu..ou
        n = n + 1
    println(f"{n}")
"#,
        "23",
    );
}

#[test]
fn a_wide_integer_range_reaches_its_maximum() {
    assert_runs_with_output(
        r#"
fn main()
    let top i128 = 170141183460469231731687303715884105727
    let below i128 = top - 2
    var last i128 = 0
    var n = 0
    for i in below..=top
        last = i
        n = n + 1
    let utop u128 = 340282366920938463463374607431768211455
    let ubelow u128 = utop - 1
    var ulast u128 = 0
    for i in ubelow..=utop
        ulast = i
        n = n + 1
    println(f"{n} {last == top} {ulast == utop}")
"#,
        "5 true true",
    );
}

/// The body of `function` as `miri build --emit mir` writes it for `program`,
/// without its header line.
fn emitted_body(program: &str, function: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("prog.mi");
    std::fs::write(&source, program).unwrap();
    let out = dir.path().join("prog");
    crate::utils::miri_cmd()
        .arg("build")
        .arg(&source)
        .arg("--out")
        .arg(&out)
        .args(["--emit", "mir"])
        .assert()
        .success();
    let mir = std::fs::read_to_string(dir.path().join("prog.mir")).unwrap();
    let header = format!("fn {function}\n");
    let start = mir
        .find(&header)
        .unwrap_or_else(|| panic!("no `{function}` in:\n{mir}"));
    let body = &mir[start + header.len()..];
    body[..body.find("\nfn ").unwrap_or(body.len())].to_string()
}

/// A loop over a `Steppable`-bounded parameter pinned to `int` is the loop
/// written over `int`: the bound adds no call to it.
#[test]
fn a_steppable_generic_at_int_lowers_to_the_integer_loop() {
    let program = r#"
fn walk<T implements Steppable>(a T, b T) int
    var passes = 0
    for x in a..b
        passes = passes + 1
    return passes

fn walk_int(a int, b int) int
    var passes = 0
    for x in a..b
        passes = passes + 1
    return passes

fn main()
    println(f"{walk(1, 4)} {walk_int(1, 4)}")
"#;
    assert_eq!(
        emitted_body(program, "miri.walk$int"),
        emitted_body(program, "miri.walk_int")
    );
}

/// `forall` runs its passes as independent indices, so a range it walks is
/// held to the same stepping rule as `for`: one whose type cannot be stepped
/// at all is refused with the same code.
#[test]
fn a_forall_over_a_range_that_cannot_be_stepped_is_refused() {
    for range in ["0.5..2.5", "false..true", "0.5 as f32..2.5 as f32"] {
        assert_compiler_error(
            &format!("fn main()\n    forall x in {range}\n        println(f\"{{x}}\")\n"),
            "[MER_TYP_086]",
        );
    }
}

/// A `forall` pass is an index the runtime may hand out in any order, while a
/// range of a non-integer `Steppable` is reached one `successor` call after
/// another: such a range is refused as a parallel construct and belongs in a
/// `for` loop.
#[test]
fn a_forall_over_a_range_stepped_by_successor_is_refused() {
    assert_compiler_error(
        "fn main()\n    forall c in \"a\"..\"c\"\n        println(c)\n",
        "[MER_TAR_005]",
    );
    assert_compiler_error(
        &format!(
            "{STEPPABLE_CLASS}\nfn main()\n    forall v in V(1)..V(3)\n        println(v.n)\n"
        ),
        "[MER_TAR_005]",
    );
    assert_compiler_error(
        "fn g<T implements Steppable>(a T, b T)\n    forall x in a..b\n        println(\"pass\")\n\nfn main()\n    g(1, 3)\n",
        "[MER_TAR_005]",
    );
}

/// The integer `forall` is unchanged by the stepping rule.
#[test]
fn a_forall_over_an_integer_range_still_runs() {
    assert_runs_with_output(
        "fn main()\n    forall i in 0..=3\n        println(f\"{i}\")\n",
        "0\n1\n2\n3",
    );
}
