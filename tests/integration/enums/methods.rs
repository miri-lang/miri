// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_method_simple_return() {
    assert_runs_with_output(
        r#"

enum Status
    Active
    Inactive

    fn is_active() bool
        return true

fn main()
    let s = Status.Active
    println(f"{s.is_active()}")
"#,
        "true",
    );
}

#[test]
fn test_enum_method_match_on_self() {
    assert_runs_with_output(
        r#"

enum Direction
    North
    South

    fn label() String
        match self
            Direction.North: "north"
            Direction.South: "south"

fn main()
    println(Direction.North.label())
    println(Direction.South.label())
"#,
        "north\nsouth",
    );
}

#[test]
fn test_enum_method_with_param() {
    assert_runs_with_output(
        r#"

enum Counter
    Value(int)

    fn add(n int) int
        match self
            Counter.Value(v): v + n

fn main()
    let c = Counter.Value(10)
    println(f"{c.add(5)}")
"#,
        "15",
    );
}

#[test]
fn test_enum_method_declaring_self_is_called_without_it() {
    assert_runs_with_output(
        r#"

enum Counter
    Value(int)

    fn add(self, n int) int
        match self
            Counter.Value(v): v + n

fn main()
    let c = Counter.Value(10)
    println(f"{c.add(5)}")
"#,
        "15",
    );
}

#[test]
fn test_must_use_user_defined_enum_discarded() {
    assert_compiler_error(
        r#"

must_use enum Token
    Valid(String)
    Invalid

fn tokenize(s String) Token
    Token.Valid(s)

fn main()
    tokenize("hello")
"#,
        "must be used",
    );
}

#[test]
fn test_must_use_user_defined_enum_bound_ok() {
    assert_runs(
        r#"

must_use enum Token
    Valid(String)
    Invalid

fn tokenize(s String) Token
    Token.Valid(s)

fn main()
    let t = tokenize("hello")
    match t
        Token.Valid(_): println("ok")
        Token.Invalid: println("bad")
"#,
    );
}

#[test]
fn test_must_use_literal_discarded() {
    assert_compiler_error(
        r#"
must_use enum Coin
    Heads
    Tails

fn main()
    Coin.Heads
"#,
        "must be used",
    );
}

#[test]
fn test_enum_private_method_is_refused_outside_the_enum() {
    assert_compiler_error(
        r#"
enum Shape
    Empty

    private fn hidden() int
        2

fn main()
    let s = Shape.Empty
    println(f"{s.hidden()}")
"#,
        "Method 'hidden' of enum 'Shape' is Private and cannot be accessed from here",
    );
}

#[test]
fn test_enum_private_method_is_refused_from_another_module() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.shapes\n",
                    "\n",
                    "fn main()\n",
                    "    let s = Shape.Empty\n",
                    "    println(f\"{s.hidden()}\")\n",
                ),
            ),
            (
                "shapes.mi",
                concat!(
                    "public enum Shape\n",
                    "    Empty\n",
                    "\n",
                    "    private fn hidden() int\n",
                    "        2\n",
                ),
            ),
        ],
        "is Private and cannot be accessed",
    );
}

#[test]
fn test_enum_private_method_is_callable_from_its_own_methods() {
    assert_runs_with_output(
        r#"
enum Shape
    Empty

    private fn hidden() int
        2

    fn doubled() int
        self.hidden() * 2

fn main()
    let s = Shape.Empty
    println(f"{s.doubled()}")
"#,
        "4",
    );
}

#[test]
fn test_enum_method_without_a_keyword_is_public_across_modules() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.shapes\n",
                    "\n",
                    "fn main()\n",
                    "    let s = Shape.Empty\n",
                    "    println(f\"{s.plain()}\")\n",
                ),
            ),
            (
                "shapes.mi",
                concat!(
                    "public enum Shape\n",
                    "    Empty\n",
                    "\n",
                    "    fn plain() int\n",
                    "        1\n",
                ),
            ),
        ],
        "1",
    );
}
