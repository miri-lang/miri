// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `@empties` marks the method that takes a value out of its receiver and
//! hands it back: the whole receiver when it names no parameter, the element
//! at the index parameter it names otherwise. It is refused, at the
//! attribute, on any method that cannot do that.

use super::utils::*;

const CODE: &str = "MER_TYP_054";

/// A collection of `int`s around `rest`, a method of it.
fn collection(rest: &str) -> String {
    format!(
        r#"use system.collections.list

class Bag
    var items [int]
    var count int
    fn init()
        self.items = List<int>()
        self.count = 0
{rest}

fn main()
    let b = Bag()
    println(f"{{b.count}}")
"#
    )
}

/// Asserts `rest` is refused at its `@empties` line, the message carrying
/// `reason`.
fn assert_refused(rest: &str, reason: &str) {
    let diagnostic = assert_refused_at(&collection(rest), CODE, "@empties");
    assert!(
        diagnostic.message.contains(reason),
        "the refusal says '{reason}': {diagnostic:#?}"
    );
}

#[test]
fn empties_naming_a_missing_parameter_is_refused() {
    assert_refused(
        r#"
    @empties("index")
    fn pull(i int) int?
        self.count = self.count - 1
        return self.items.remove_at(i)
"#,
        "names no parameter `index`",
    );
}

#[test]
fn empties_naming_a_parameter_that_is_not_an_int_is_refused() {
    assert_refused(
        r#"
    @empties("i")
    fn pull(i String) int?
        self.count = self.count - 1
        return None
"#,
        "`i` is not an `int`",
    );
}

#[test]
fn empties_on_a_method_returning_no_optional_is_refused() {
    assert_refused(
        r#"
    @empties("i")
    fn pull(i int) int
        self.count = self.count - 1
        return 0
"#,
        "returns no optional",
    );
}

#[test]
fn empties_on_a_method_not_writing_its_receiver_is_refused() {
    assert_refused(
        r#"
    @empties("i")
    fn peek(i int) int?
        return None
"#,
        "never writes its receiver",
    );
}

#[test]
fn empties_on_a_static_method_is_refused() {
    assert_refused(
        r#"
    @empties("i")
    static fn make(i int) int?
        return None
"#,
        "is not an instance method",
    );
}

#[test]
fn empties_on_a_method_taking_another_parameter_is_refused() {
    assert_refused(
        r#"
    @empties("i")
    fn pull(i int, fallback int) int?
        self.count = self.count - 1
        return self.items.remove_at(i)
"#,
        "takes a parameter besides `i`",
    );
}

#[test]
fn empties_on_a_free_function_is_refused() {
    let diagnostic = assert_refused_at(
        r#"
@empties
fn pull() int?
    return None

fn main()
    println(f"{pull()}")
"#,
        CODE,
        "@empties",
    );
    assert!(
        diagnostic.message.contains("is not an instance method"),
        "the refusal says the function is no method: {diagnostic:#?}"
    );
}

#[test]
fn empties_on_a_method_that_takes_out_an_element_compiles() {
    assert_runs_with_output(
        &collection(
            r#"
    @empties("i")
    fn pull(i int) int?
        self.count = self.count - 1
        return self.items.remove_at(i)
"#,
        ),
        "0",
    );
}

/// An enum method carries attributes as a class method does: the mark on an
/// enum's method that empties it whole holds.
#[test]
fn empties_on_an_enum_method_that_empties_it_whole_compiles() {
    assert_runs_with_output(
        r#"
enum Cell
    Empty
    Full(int)

    @empties
    fn drain() int?
        let held = match self
            Cell.Full(x): Some(x)
            Cell.Empty: None
        self = Cell.Empty
        return held

fn main()
    var c = Cell.Full(3)
    println(f"{c.drain()}")
"#,
        "Some(3)",
    );
}

#[test]
fn an_attribute_on_an_enum_variant_is_refused() {
    assert_compiler_error(
        r#"
enum Cell
    @empties
    Empty
    Full(int)

fn main()
    println("x")
"#,
        "Unsupported Attribute Target",
    );
}
