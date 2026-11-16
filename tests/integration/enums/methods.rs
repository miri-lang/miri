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

#[test]
fn two_generic_enums_hand_a_generic_function_an_out_place_of_their_own_shape() {
    assert_heap_guard_output(
        r#"
use system.io

fn pick<T>(place out T, v T) T
    return v

enum A<T>
    X(T)
    Y

    fn f(v A<T>) A<T>
        return pick(self, v)

enum B<T>
    P(T)
    Q

    fn g(v T?) T?
        var o = v
        return pick(o, v)

fn main()
    var a A<String> = A.X("a")
    let r = a.f(A.Y)
    match r
        A.X(s): println(s)
        A.Y: println("y")
    var b B<String> = B.Q
    let s = b.g(Some("z"))
    println(s ?? "-")
"#,
        "y\nz",
    );
}

#[test]
fn a_generic_enum_runs_its_drop_hook_at_each_instantiation() {
    assert_heap_guard_output(
        r#"
use system.io
use system.ops

enum Guard<T implements Equatable>
    Held(T, T)
    Empty

    fn drop(self)
        match self
            Guard.Held(a, b): println(f"dropped {a.equals(b)}")
            Guard.Empty: println("dropped empty")

fn main()
    let g Guard<String> = Guard.Held("x", "y")
    let h Guard<i32> = Guard.Held(7, 7)
    let e Guard<int> = Guard.Empty
    println("live")
"#,
        "live\ndropped empty\ndropped true\ndropped false",
    );
}

#[test]
fn a_generic_enum_drop_hook_builds_a_list_of_its_parameter() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

enum B<T>
    P(T)
    Q

    fn drop(self)
        let l = List<T>()
        println(f"bdrop {l.length()}")

fn main()
    let b B<i8> = B.P(1)
    println("live")
"#,
        "live\nbdrop 0",
    );
}

#[test]
fn a_generic_enum_drop_hook_beside_a_called_method_builds_a_list_of_its_parameter() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

enum B<T>
    P(T)
    Q

    fn drop(self)
        let l = List<T>()
        println(f"bdrop {l.length()}")

    fn ok() bool
        return true

fn main()
    let b B<i8> = B.P(1)
    println(f"live {b.ok()}")
"#,
        "live true\nbdrop 0",
    );
}

#[test]
fn a_generic_enum_built_in_a_generic_function_runs_a_drop_hook_that_builds_a_list() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

enum B<T>
    P(T)
    Q

    fn drop(self)
        let l = List<T>()
        println(f"bdrop {l.length()}")

fn hold<T>(x T)
    let b B<T> = B.P(x)
    println("held")

fn main()
    hold(2 as i16)
    println("live")
"#,
        "held\nbdrop 0\nlive",
    );
}

#[test]
fn a_generic_enum_held_in_a_list_runs_a_drop_hook_that_builds_a_list() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

enum B<T>
    P(T)
    Q

    fn drop(self)
        let l = List<T>()
        println(f"bdrop {l.length()}")

fn main()
    var xs = List<B<i16>>()
    xs.push(B.P(1))
    println("live")
"#,
        "live\nbdrop 0",
    );
}
