// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_to_upper() {
    assert_runs_with_output(
        r#"

let s = "hello"
print(s.to_upper())
"#,
        "HELLO",
    );
}

#[test]
fn test_string_to_lower() {
    assert_runs_with_output(
        r#"

let s = "HELLO"
print(s.to_lower())
"#,
        "hello",
    );
}

#[test]
fn test_string_trim() {
    assert_runs_with_output(
        r#"

let s = "  hello  "
print(s.trim())
"#,
        "hello",
    );
}

#[test]
fn test_string_trim_start() {
    assert_runs_with_output(
        r#"

let s = "  hello"
print(s.trim_start())
"#,
        "hello",
    );
}

#[test]
fn test_string_trim_end() {
    assert_runs_with_output(
        r#"

let s = "hello  "
print(s.trim_end())
"#,
        "hello",
    );
}

#[test]
fn test_string_contains() {
    assert_runs_with_output(
        r#"

let s = "hello world"
if s.contains("world")
    println("found")
else
    println("not found")
"#,
        "found",
    );
}

#[test]
fn test_string_starts_with() {
    assert_runs_with_output(
        r#"

let s = "hello world"
if s.starts_with("hello")
    println("yes")
else
    println("no")
"#,
        "yes",
    );
}

#[test]
fn test_string_ends_with() {
    assert_runs_with_output(
        r#"

let s = "hello world"
if s.ends_with("world")
    println("yes")
else
    println("no")
"#,
        "yes",
    );
}

#[test]
fn test_string_replace() {
    assert_runs_with_output(
        r#"

let s = "hello world"
print(s.replace("world", "miri"))
"#,
        "hello miri",
    );
}

#[test]
fn test_string_substring() {
    assert_runs_with_output(
        r#"

let s = "hello"
print(s.substring(1, 4))
"#,
        "ell",
    );
}

#[test]
fn test_string_is_empty() {
    assert_runs_with_output(
        r#"

let s = ""
if s.is_empty()
    println("empty")
else
    println("not empty")
"#,
        "empty",
    );
}

#[test]
fn test_string_length_method() {
    assert_runs_with_output(
        r#"

let s = "hello"
println(f"{s.length()}")
"#,
        "5",
    );
}

#[test]
fn test_string_unicode_length_and_size() {
    assert_runs_with_output(
        r#"

let s = "🦀"
println(f"length: {s.length()}")
println(f"size: {s.size()}")
"#,
        "length: 1\nsize: 4",
    );
}

#[test]
fn test_string_chained_methods() {
    assert_runs_with_output(
        r#"

let s = "  HELLO  "
print(s.trim().to_lower())
"#,
        "hello",
    );
}

#[test]
fn test_substring_drops_a_leading_character_of_non_ascii_text() {
    // The idiom an agent writes to drop a byte-order mark or a first letter.
    // With byte offsets it returned "" on any text whose first character is
    // wider than one byte.
    assert_runs_with_output(
        r#"
fn main()
    var text = "Élan"
    text = text + " vital"
    println(text.substring(1, text.length()))
    println(text.substring(0, text.length() - 1))
"#,
        "lan vital\nÉlan vita",
    );
}

#[test]
fn test_substring_past_the_end_stops_the_program() {
    assert_runtime_error(
        r#"
fn main()
    var end = 3
    end = end * 3
    println("héllo".substring(1, end))
"#,
        "substring range out of bounds: [1, 9) but the string has 5 characters",
    );
}

#[test]
fn test_substring_with_an_end_before_its_start_stops_the_program() {
    assert_runtime_error(
        r#"
fn main()
    var start = 1
    start = start + 2
    println("hello".substring(start, 1))
"#,
        "substring range out of bounds: [3, 1)",
    );
}

#[test]
fn test_a_constant_substring_past_the_end_is_refused_at_compile_time() {
    assert_compiler_error(
        r#"
const GREETING = "héllo"

fn main()
    println(GREETING.substring(1, 9))
"#,
        "Range out of bounds: end 9 but the length is 5",
    );
    assert_compiler_error(
        r#"
fn main()
    let word = "abc"
    println(word.substring(2, 1))
"#,
        "Range end 1 is before its start 2",
    );
    assert_compiler_error(
        r#"
fn main()
    println("xy".substring(-1, 1))
"#,
        "Range start -1 is negative",
    );
}

#[test]
fn test_a_constant_substring_inside_the_text_compiles() {
    assert_runs_with_output(
        r#"
fn main()
    let word = "日本語"
    println(word.substring(0, 3))
    println(word.substring(3, 3))
"#,
        "日本語\n",
    );
}

#[test]
fn test_a_reassignable_text_is_not_judged_at_compile_time() {
    // A `var` can hold a longer text by the time the call runs, so only the
    // runtime can say whether the range fits.
    assert_runs_with_output(
        r#"
fn main()
    var word = "ab"
    word = word + "cdef"
    println(word.substring(1, 5))
"#,
        "bcde",
    );
}

/// The doc comment directly above `fn_name` in `source`.
fn doc_of(source: &str, fn_name: &str) -> String {
    let lines: Vec<&str> = source.lines().collect();
    let at = lines
        .iter()
        .position(|line| {
            line.trim_start()
                .starts_with(&format!("public fn {}(", fn_name))
        })
        .unwrap_or_else(|| panic!("no public fn {}", fn_name));
    lines[..at]
        .iter()
        .rev()
        .take_while(|line| {
            let line = line.trim_start();
            line.starts_with("///") || line.starts_with('@')
        })
        .map(|line| line.trim_start().trim_start_matches("///").trim())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_every_position_the_string_api_takes_or_gives_is_in_characters() {
    // A mechanical audit of the published unit of every position in the text
    // API: what `length()` counts, `substring`, `element_at`, `code_at` and a
    // regex match's `start`/`end` must count too, and only `size()` speaks in
    // bytes. A method added with a byte position fails here until it is
    // converted or deliberately named for bytes.
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib/system");
    let string = std::fs::read_to_string(root.join("string.mi")).expect("string.mi");
    let text = std::fs::read_to_string(root.join("text.mi")).expect("text.mi");
    for (source, name) in [
        (&string, "length"),
        (&string, "substring"),
        (&string, "element_at"),
        (&string, "code_at"),
        (&text, "start"),
        (&text, "end"),
    ] {
        let doc = doc_of(source, name).to_lowercase();
        assert!(
            doc.contains("character"),
            "{} does not say it counts characters: {}",
            name,
            doc
        );
    }
    assert!(
        doc_of(&string, "size").to_lowercase().contains("byte"),
        "size() is the one byte count, and must say so"
    );
    let byte_methods: Vec<&str> = string
        .lines()
        .filter(|line| line.trim_start().starts_with("public fn "))
        .filter_map(|line| {
            let name = line.trim_start().trim_start_matches("public fn ");
            name.split('(').next()
        })
        .filter(|name| {
            let doc = doc_of(&string, name).to_lowercase();
            doc.contains("byte") && !doc.contains("character")
        })
        .collect();
    assert_eq!(
        byte_methods,
        ["size"],
        "a String method other than size() documents a byte position"
    );
}
