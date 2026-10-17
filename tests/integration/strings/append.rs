// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `s = s + t` and `s += t` append into the string a local holds when nothing
//! else holds it, instead of copying all of it into a new one.

use super::utils::{assert_runs_untracked, assert_runs_with_output};

/// The raw allocations a program makes, as `MIRI_ALLOC_COUNT` reports them.
fn raw_allocations(code: &str) -> usize {
    let result = crate::utils::miri_run_with_env(code, "MIRI_ALLOC_COUNT", "1");
    assert!(result.success, "{}", result.output());
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

fn builder(pieces: usize, append: &str) -> String {
    format!(
        "use system.io\n\nfn main()\n    var s = \"\"\n    var i = 0\n    while i < {pieces}\n        {append}\n        i = i + 1\n    println(f\"{{s.length()}}\")\n"
    )
}

/// Building a string piece by piece grows its buffer by doubling, so twice the
/// pieces cost one more allocation or so, not twice as many.
#[test]
fn test_appending_to_an_unshared_string_allocates_logarithmically() {
    for append in ["s += \"ab\"", "s = s + \"ab\""] {
        let few = raw_allocations(&builder(2000, append));
        let many = raw_allocations(&builder(4000, append));
        assert!(
            many <= few + 2,
            "`{append}`: 2000 pieces allocated {few} times, 4000 allocated {many}"
        );
    }
}

#[test]
fn test_appending_leaves_another_holder_of_the_string_unchanged() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var s = "a"
    s += "b"
    let kept = s
    s += "c"
    s = s + "d"
    println(f"{s} {kept}")
"#,
        "abcd ab",
    );
}

#[test]
fn test_a_string_appended_to_itself_doubles() {
    assert_runs_untracked(
        r#"
use system.io

fn main()
    var s = "xy"
    s += "z"
    s = s + s
    s += s
    println(f"{s} {s.length()}")
"#,
        "xyzxyzxyzxyz 12",
    );
}

#[test]
fn test_a_long_build_keeps_every_piece_in_order() {
    assert_runs_untracked(
        r#"
use system.io

fn main()
    var s = ""
    var i = 0
    while i < 1000
        s += f"{i % 10}"
        i = i + 1
    println(f"{s.length()} {s.substring(0, 12)} {s.substring(990, 1000)}")
"#,
        "1000 012345678901 0123456789",
    );
}

#[test]
fn test_appending_to_a_string_literal_copies_the_literal() {
    assert_runs_with_output(
        r#"
use system.io

fn greet() String
    var s = "hello"
    s += ", world"
    return s

fn main()
    println(greet())
    println(greet())
"#,
        "hello, world\nhello, world",
    );
}

#[test]
fn test_a_chain_of_pieces_appends_each_in_order() {
    assert_runs_with_output(
        r#"
use system.io

fn label(n int) String: f"<{n}>"

fn main()
    var s = "["
    var i = 0
    while i < 3
        s = s + label(i) + "," + s.substring(0, 1)
        i = i + 1
    println(s)
"#,
        "[<0>,[<1>,[<2>,[",
    );
}

/// A formatted string grows its own buffer as it adds each part, and leaves
/// every string a part was read from — a field, an element, a map value, a
/// binding — as it was.
#[test]
fn test_a_formatted_string_leaves_the_strings_it_reads_unchanged() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

struct Person
    name String

class Box
    var label String
    fn init(label String)
        self.label = label

fn main()
    let p = Person(name: "ann")
    let l = List(["x", "y"])
    let m = {"k": "v"}
    let bx = Box("lbl")
    let s = "plain"
    let t = (s, "two")
    let built = f"{p.name}!{l[0]}?{m['k']}#{bx.label}*{s}+{t.1}~"
    println(f"{built} | {p.name} {l[0]} {m['k']} {bx.label} {s} {t.1}")
"#,
        "ann!x?v#lbl*plain+two~ | ann x v lbl plain two",
    );
}

/// A formatted string appends each integer's digits straight onto the text
/// it is building, for every integer width and sign, and an integer first or
/// after one of the program's own strings is written the same way.
#[test]
fn test_formatted_integers_of_every_width_keep_their_text() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let name = "n"
    let small i8 = -128
    let short i16 = 32767
    let word i32 = -2147483648
    let wide = -9223372036854775807 - 1
    let byte u8 = 255
    let unsigned u32 = 4294967295
    let big u64 = 18446744073709551615
    println(f"{small}|{short}|{word}|{wide}|{byte}|{unsigned}|{big}")
    println(f"{name}{wide}{name}")
    println(f"{0}{-1}:{name}")
"#,
        "-128|32767|-2147483648|-9223372036854775808|255|4294967295|18446744073709551615\nn-9223372036854775808n\n0-1:n",
    );
}

/// A formatted string appends each number's digits onto its own text, with no
/// string of the digits allocated on the way: a piece holding two numbers
/// costs the two allocations of its own text, not two more per number.
#[test]
fn test_formatted_numbers_allocate_no_string_of_their_own() {
    let program = format!(
        "use system.io\n\nfn main()\n    var total = 0\n    var i = 0\n    while i < 1000\n        let piece = f\"<{{i}}:{{i * 2}}>\"\n        total = total + piece.length()\n        i = i + 1\n    println(f\"{{total}}\")\n"
    );
    let allocations = raw_allocations(&program);
    assert!(
        allocations <= 2 * 1000 + 10,
        "1000 pieces of two numbers allocated {allocations} times"
    );
}
