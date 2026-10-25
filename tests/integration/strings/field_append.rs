// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `obj.f = obj.f + t` and `obj.f += t` on a `String` field append into the
//! string the field holds when nothing else holds it, instead of copying all
//! of it into a new one, and leave every other holder of the old value with
//! the value it had.

use super::utils::{assert_compiler_error, assert_heap_guard_output, assert_runs_with_output};

/// The raw allocations a program makes, as `MIRI_ALLOC_COUNT` reports them,
/// after checking it printed `expected`.
fn raw_allocations(code: &str, expected: &str) -> usize {
    let result = crate::utils::miri_run_with_env(code, "MIRI_ALLOC_COUNT", "1");
    assert!(result.success, "{}", result.output());
    assert_eq!(result.stdout.trim(), expected, "{}", result.output());
    let line = result
        .stderr
        .lines()
        .find(|line| line.starts_with("MIRI_ALLOC_COUNT:"))
        .unwrap_or_else(|| panic!("no allocation count in:\n{}", result.stderr));
    line.split("raw=")
        .nth(1)
        .and_then(|rest| rest.split([' ', ')']).next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("no raw count in: {line}"))
}

/// Checks that a program `build`s with `pieces` appends of `"ab"` grows its
/// string by doubling — twice the pieces cost one more allocation or so, not
/// twice as many — prints the length it built, and runs clean under the heap
/// guard.
fn assert_appends_allocate_logarithmically(build: impl Fn(usize) -> String, append: &str) {
    let few = raw_allocations(&build(2000), "4000");
    let many = raw_allocations(&build(4000), "8000");
    assert!(
        many <= few + 2,
        "`{append}`: 2000 pieces allocated {few} times, 4000 allocated {many}"
    );
    assert_heap_guard_output(&build(100), "200");
}

/// A program appending `pieces` times through `append`, a statement of the
/// method `add` of a class holding the text in `self.text`.
fn class_builder(pieces: usize, append: &str) -> String {
    format!(
        "use system.io\n\nclass Writer\n    var text String\n\n    fn add(piece String)\n        {append}\n\nfn main()\n    var w = Writer(text: \"\")\n    var i = 0\n    while i < {pieces}\n        w.add(\"ab\")\n        i = i + 1\n    println(f\"{{w.text.length()}}\")\n"
    )
}

/// A program appending `pieces` times to the text of the struct in the list
/// `l`, through `append`.
fn element_builder(pieces: usize, append: &str) -> String {
    format!(
        "use system.io\n\nstruct Doc\n    text String\n\nfn main()\n    var l = [Doc(text: \"\")]\n    var i = 0\n    var k = 0\n    while i < {pieces}\n        {append}\n        i = i + 1\n    println(f\"{{l[0].text.length()}}\")\n"
    )
}

#[test]
fn test_appending_to_a_string_field_allocates_logarithmically() {
    for append in ["self.text += piece", "self.text = self.text + piece"] {
        assert_appends_allocate_logarithmically(|pieces| class_builder(pieces, append), append);
    }
}

#[test]
fn test_appending_to_an_element_field_allocates_logarithmically() {
    for append in ["l[k].text += \"ab\"", "l[0].text = l[0].text + \"ab\""] {
        assert_appends_allocate_logarithmically(|pieces| element_builder(pieces, append), append);
    }
}

#[test]
fn test_no_append_leaves_the_field_as_it_was() {
    assert_runs_with_output(&class_builder(0, "self.text += piece"), "0");
    assert_runs_with_output(&class_builder(0, "self.text = self.text + piece"), "0");
}

#[test]
fn test_compound_append_to_a_class_field() {
    let code = r#"
use system.io

class Writer
    var text String

    fn add(piece String)
        self.text += piece

fn main()
    var w = Writer(text: "")
    w.add("a")
    let one = w.text
    w.add("b")
    let two = w.text
    var i = 0
    while i < 1000
        w.add("c")
        i = i + 1
    println(f"{one} {two} {w.text.length()}")
"#;
    assert_runs_with_output(code, "a ab 1002");
    assert_heap_guard_output(code, "a ab 1002");
}

#[test]
fn test_concatenation_of_several_pieces_onto_a_class_field() {
    let code = r#"
use system.io

class Writer
    var text String

    fn add(a String, b String)
        self.text = self.text + a + b

fn main()
    var w = Writer(text: "x")
    w.add("a", "b")
    w.add("c", "d")
    println(w.text)
"#;
    assert_runs_with_output(code, "xabcd");
    assert_heap_guard_output(code, "xabcd");
}

#[test]
fn test_a_field_appended_to_itself_doubles() {
    let code = r#"
use system.io

class Writer
    var text String

    fn double()
        self.text = self.text + self.text

    fn double_again()
        self.text += self.text

fn main()
    var w = Writer(text: "t")
    w.double()
    let once = w.text
    w.double()
    let twice = w.text
    w.double_again()
    println(f"{once} {twice} {w.text}")
"#;
    assert_runs_with_output(code, "tt tttt tttttttt");
    assert_heap_guard_output(code, "tt tttt tttttttt");
}

#[test]
fn test_a_binding_taken_before_the_append_keeps_the_old_text() {
    let code = r#"
use system.io

class Writer
    var text String

    fn add(piece String) String
        let before = self.text
        self.text += piece
        return before

fn main()
    var w = Writer(text: "old")
    let before = w.add("x")
    println(f"{before} {w.text}")
"#;
    assert_runs_with_output(code, "old oldx");
    assert_heap_guard_output(code, "old oldx");
}

#[test]
fn test_appending_to_a_shared_struct_leaves_the_other_copy_unchanged() {
    let code = r#"
use system.io

struct Doc
    t String

fn main()
    var s = Doc(t: "a")
    var s2 = s
    s.t += "x"
    s.t = s.t + "y"
    println(f"{s.t} {s2.t}")
"#;
    assert_runs_with_output(code, "axy a");
    assert_heap_guard_output(code, "axy a");
}

#[test]
fn test_appending_to_an_element_of_a_shared_list_leaves_the_other_list_unchanged() {
    let code = r#"
use system.io

struct Doc
    t String

fn main()
    var l = [Doc(t: "a"), Doc(t: "b")]
    var l2 = l
    var i = 1
    l[i].t += "x"
    l[0].t = l[0].t + "y"
    let changed = l[1].t
    let kept = l2[1].t
    let first = l[0].t
    let first_kept = l2[0].t
    println(f"{changed} {kept} {first} {first_kept}")
"#;
    assert_runs_with_output(code, "bx b ay a");
    assert_heap_guard_output(code, "bx b ay a");
}

#[test]
fn test_a_piece_that_replaces_the_field_appends_to_the_old_text() {
    let code = r#"
use system.io

class Writer
    var text String

    fn replace() String
        self.text = "z"
        return "p"

    fn add()
        self.text = self.text + self.replace()

fn main()
    var w = Writer(text: "old")
    w.add()
    println(w.text)
"#;
    assert_runs_with_output(code, "oldp");
    assert_heap_guard_output(code, "oldp");
}

#[test]
fn test_a_later_piece_reads_the_field_before_the_append() {
    let code = r#"
use system.io

class Writer
    var text String

    fn first() String
        return "f"

    fn second() String
        return "[" + self.text + "]"

    fn add()
        self.text = self.text + self.first() + self.second()

fn main()
    var w = Writer(text: "o")
    w.add()
    println(w.text)
"#;
    assert_runs_with_output(code, "of[o]");
    assert_heap_guard_output(code, "of[o]");
}

#[test]
fn test_appending_to_an_optional_string_field() {
    let code = r#"
use system.io

class Writer
    var text String?

    fn add(piece String)
        self.text = (self.text ?? "") + piece

fn main()
    var w = Writer(text: None)
    w.add("a")
    w.add("b")
    let shown = w.text ?? "none"
    println(shown)
"#;
    assert_runs_with_output(code, "ab");
    assert_heap_guard_output(code, "ab");
}

#[test]
fn test_compound_append_to_an_optional_string_field_is_refused() {
    assert_compiler_error(
        r#"
class Writer
    var text String?

    fn add(piece String)
        self.text += piece

fn main()
    var w = Writer(text: None)
    w.add("a")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_appending_to_a_field_reached_through_nested_objects() {
    let code = r#"
use system.io

class Writer
    var text String

class Holder
    var w Writer

fn make() Writer
    return Writer(text: "m")

fn main()
    var h = Holder(w: make())
    h.w.text += "x"
    h.w.text = h.w.text + "y" + "z"
    println(h.w.text)
"#;
    assert_runs_with_output(code, "mxyz");
    assert_heap_guard_output(code, "mxyz");
}

#[test]
fn test_appending_to_a_generic_field_held_at_string() {
    let code = r#"
use system.io

class Cell<T>
    var value T

fn main()
    var c = Cell<String>(value: "g")
    c.value += "h"
    c.value = c.value + "i"
    println(c.value)
"#;
    assert_runs_with_output(code, "ghi");
    assert_heap_guard_output(code, "ghi");
}

#[test]
fn test_appending_to_a_field_of_a_map_entry() {
    let code = r#"
use system.io

struct Doc
    t String

class W
    var t String

fn main()
    var m = {"a": Doc(t: "x")}
    var m2 = m
    m["a"].t += "y"
    m["a"].t = m["a"].t + "z"
    let k = "a"
    m[k].t += "q"
    var c = {"a": W(t: "c")}
    c["a"].t += "d"
    let one = m["a"].t
    let two = m2["a"].t
    let three = c["a"].t
    println(f"{one} {two} {three}")
"#;
    assert_runs_with_output(code, "xyzq x cd");
    assert_heap_guard_output(code, "xyzq x cd");
}

#[test]
fn test_appending_empty_strings_to_a_field() {
    let code = r#"
use system.io

class Writer
    var text String

    fn add(t String)
        self.text += ""
        self.text = self.text + t + "" + ""
        self.text += ""

fn main()
    var w = Writer(text: "e")
    w.add("f")
    w.add("")
    println(f"{w.text} {w.text.length()}")
"#;
    assert_runs_with_output(code, "ef 2");
    assert_heap_guard_output(code, "ef 2");
}

#[test]
fn test_compound_assignment_to_an_int_field_is_unaffected() {
    let code = r#"
use system.io

class Counter
    var n int

    fn bump()
        self.n += 1
        self.n = self.n + 2

struct Point
    n int

fn main()
    var c = Counter(n: 0)
    c.bump()
    var p = Point(n: 5)
    p.n += 1
    p.n = p.n + 2
    println(f"{c.n} {p.n}")
"#;
    assert_runs_with_output(code, "3 8");
    assert_heap_guard_output(code, "3 8");
}

#[test]
fn test_appending_to_a_class_field_is_seen_through_every_reference() {
    let code = r#"
use system.io

class Writer
    var t String

fn main()
    var a = Writer(t: "a")
    var a2 = a
    a.t += "x"
    a.t = a.t + "y"
    println(f"{a.t} {a2.t}")
"#;
    assert_runs_with_output(code, "axy axy");
    assert_heap_guard_output(code, "axy axy");
}

#[test]
fn test_a_sum_starting_at_another_objects_field_is_a_plain_store() {
    let code = r#"
use system.io

class Writer
    var t String

fn main()
    var a = Writer(t: "a")
    var b = Writer(t: "b")
    a.t = b.t + "y"
    println(f"{a.t} {b.t}")
"#;
    assert_runs_with_output(code, "by b");
    assert_heap_guard_output(code, "by b");
}

#[test]
fn test_a_sum_starting_at_another_element_is_a_plain_store() {
    let code = r#"
use system.io

struct Doc
    t String

fn main()
    var l = [Doc(t: "a"), Doc(t: "b")]
    var i = 0
    var j = 1
    l[i].t = l[j].t + "x"
    let first = l[0].t
    let second = l[1].t
    println(f"{first} {second}")
"#;
    assert_runs_with_output(code, "bx b");
    assert_heap_guard_output(code, "bx b");
}

#[test]
fn test_prepending_to_a_field() {
    let code = r#"
use system.io

class Writer
    var t String

    fn prepend(x String)
        self.t = x + self.t

fn main()
    var w = Writer(t: "t")
    w.prepend("x")
    w.prepend("y")
    println(w.t)
"#;
    assert_runs_with_output(code, "yxt");
    assert_heap_guard_output(code, "yxt");
}

#[test]
fn test_appending_through_a_computed_or_member_index() {
    let code = r#"
use system.io
use system.collections.list

struct Doc
    t String

class Book
    var l [Doc]
    var i int

    fn add(x String)
        self.l[self.i].t = self.l[self.i].t + x

fn main()
    var l = [Doc(t: "a"), Doc(t: "b")]
    var i = 1
    l[i + 0].t = l[i + 0].t + "x"
    var book = Book(l: List([Doc(t: "c"), Doc(t: "d")]), i: 0)
    book.add("y")
    let second = l[1].t
    let first = book.l[0].t
    println(f"{second} {first}")
"#;
    assert_runs_with_output(code, "bx cy");
    assert_heap_guard_output(code, "bx cy");
}
