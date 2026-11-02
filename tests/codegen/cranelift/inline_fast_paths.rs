// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The code generator inlines the common case of a few runtime functions: a
//! list push with room to spare, a string append, a byte read, the test for
//! a shared value, and the copy a write to a shared value makes. Each inline
//! copy reads the header words and count rules the runtime function does.
//! A build with its checks compiled out takes the inline paths; a build run
//! with the leak check takes the runtime functions, and checks that every
//! allocation is balanced. One program built both ways must print the same.

use crate::utils::miri_cmd;
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// Pushes past a list's capacity, appends into owned, unowned and non-ASCII
/// strings and reads them back by code point, writes to a shared list and
/// through a receiver, and keeps literals in lists.
const FAST_PATHS: &str = r#"
use system.io
use system.collections.list

class Bag
    var items [int]

    fn init(items [int])
        self.items = items

    fn add(x int)
        self.items.push(x)

fn code(s String, index int) int
    match s.code_at(index)
        Some(c): c
        None: -1

fn main()
    var grown = List<int>()
    var i = 0
    while i < 100
        grown.push(i)
        i += 1
    var total = 0
    for x in grown
        total += x
    println(f"push {grown.length()} {total} {grown[99]}")

    var literal = "lit"
    literal += "x"
    println(f"unowned {literal} {code(literal, 3)}")

    var accented = "é"
    accented += "ab"
    println(f"non-ascii {accented} {code(accented, 0)} {code(accented, 2)}")

    var ascii = "abc"
    ascii += "é"
    ascii += "z"
    ascii += "y"
    ascii += "x"
    println(f"turned {ascii} {code(ascii, 3)} {code(ascii, 6)} {ascii.length()}")

    var built = ""
    var n = 0
    while n < 40
        built += "ab"
        n += 1
    println(f"built {built.length()} {code(built, 39)}")

    var first = List([1, 2, 3])
    var second = first
    second.push(4)
    second[0] = 9
    println(f"shared {first.length()} {first[0]} {second.length()} {second[0]}")

    let word = "same"
    var words = List<String>()
    words.push(word)
    words.push(word)
    words.push("same")
    println(f"literals {words.length()} {words[0]} {words[2]}")

    var kept = List([1, 2])
    let bag = Bag(kept)
    bag.add(3)
    kept.push(7)
    println(f"receiver {kept.length()} {bag.items.length()} {kept[2]} {bag.items[2]}")
"#;

/// Builds `source` with `flags` and runs it with `env` set, returning what it
/// printed. Fails on a nonzero exit, which a leak or an unbalanced release
/// under the leak check is.
fn run_built(source: &str, flags: &[&str], env: &[(&str, &str)], dir: &Path) -> String {
    let mut file = tempfile::Builder::new().suffix(".mi").tempfile().unwrap();
    write!(file, "{source}").unwrap();
    let out = dir.join("fast_paths");
    miri_cmd()
        .arg("build")
        .arg(file.path())
        .args(flags)
        .arg("--out")
        .arg(&out)
        .assert()
        .success();
    let run = Command::new(&out)
        .envs(env.iter().copied())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout).into_owned();
    assert!(
        run.status.success(),
        "{flags:?} build failed: {}\n{stdout}",
        String::from_utf8_lossy(&run.stderr)
    );
    stdout
}

#[test]
fn test_inline_fast_paths_print_what_the_runtime_functions_print() {
    let dir = tempfile::tempdir().unwrap();
    let checked = run_built(FAST_PATHS, &[], &[("MIRI_LEAK_CHECK", "1")], dir.path());
    let inline = run_built(FAST_PATHS, &["--release"], &[], dir.path());
    assert_eq!(
        checked,
        "push 100 4950 99\n\
         unowned litx 120\n\
         non-ascii éab 233 98\n\
         turned abcézyx 233 120 7\n\
         built 80 98\n\
         shared 3 1 4 9\n\
         literals 3 same same\n\
         receiver 3 3 7 3\n"
    );
    assert_eq!(inline, checked, "the inline paths printed otherwise");
}
