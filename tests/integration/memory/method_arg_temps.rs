// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for argument temporaries at a method call. A managed argument built at
// the call site — a concatenation, another call's result — lives in a temp that
// no scope owns, so the call site is the only place that can release it. A
// plain function call already does; a method call has to release the same way,
// or every such call strands one allocation.

use super::super::utils::*;

#[test]
fn test_temp_argument_to_a_user_method_is_released() {
    assert_runs_with_output(
        r#"
class Ruler
    public fn width(text String) int: text.length()

fn main()
    let ruler = Ruler()
    let width = ruler.width("ab" + "cd")
    println(f"{width}")
"#,
        "4",
    );
}

#[test]
fn test_temp_argument_to_a_generic_method_is_released() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var scores = Map<String, int>()
    scores.set("ke" + "y", 7)
    match scores.get("ke" + "y")
        Some(score): println(f"{score}")
        None: println("missing")
"#,
        "7",
    );
}

#[test]
fn test_call_result_argument_to_a_method_is_released() {
    assert_runs_with_output(
        r#"
class Ruler
    public fn width(text String) int: text.length()

fn shout(word String) String: word + "!"

fn main()
    let ruler = Ruler()
    let width = ruler.width(shout("hi"))
    println(f"{width}")
"#,
        "3",
    );
}

#[test]
fn test_repeated_temp_arguments_in_a_loop_stay_balanced() {
    assert_runs_with_output(
        r#"
class Ruler
    public fn width(text String) int: text.length()

fn main()
    let ruler = Ruler()
    var index = 0
    var total = 0
    while index < 50
        total = total + ruler.width("ab" + "cd")
        index = index + 1
    println(f"{total}")
"#,
        "200",
    );
}

#[test]
fn test_named_local_argument_to_a_method_survives_the_call() {
    assert_runs_with_output(
        r#"
class Ruler
    public fn width(text String) int: text.length()

fn main()
    let ruler = Ruler()
    let text = "ab" + "cd"
    let width = ruler.width(text)
    println(f"{text} {width}")
"#,
        "abcd 4",
    );
}

#[test]
fn test_managed_argument_stored_by_the_callee_outlives_the_call() {
    assert_runs_with_output(
        r#"
class Label
    public var text String

    public fn init()
        self.text = ""

    public fn adopt(value String)
        self.text = value

fn main()
    let label = Label()
    label.adopt("ab" + "cd")
    println(label.text)
"#,
        "abcd",
    );
}
