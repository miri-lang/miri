// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_simple_payload_equality() {
    assert_runs_with_output(
        r#"
enum Color
    Red
    Blue
    Green

fn main()
    if Color.Red == Color.Red
        println("Red == Red")
    if Color.Red != Color.Blue
        println("Red != Blue")
"#,
        "Red == Red\nRed != Blue",
    );
}

#[test]
fn test_enum_int_payload_equality() {
    assert_runs_with_output(
        r#"
enum Message
    Text(int)
    Number(int)

fn main()
    let m1 = Message.Text(42)
    let m2 = Message.Text(42)
    let m3 = Message.Text(43)
    
    if m1 == m2
        println("Text(42) == Text(42)")
    if m1 != m3
        println("Text(42) != Text(43)")
"#,
        "Text(42) == Text(42)\nText(42) != Text(43)",
    );
}

#[test]
fn test_enum_string_payload_equality() {
    assert_runs_with_output(
        r#"
// Built at runtime rather than written as a literal: two equal literals may
// share one allocation, so a literal would let a pointer comparison pass this
// test without ever comparing the characters.
fn s() String
    return "h" + "i"

enum Msg
    Text(String)
    Num(int)

fn main()
    let m1 = Msg.Text(s())
    let m2 = Msg.Text(s())
    
    if m1 == m2
        println("Text(s()) == Text(s()) true")
    else
        println("Text(s()) == Text(s()) false")
"#,
        "Text(s()) == Text(s()) true",
    );
}

#[test]
fn test_result_ok_equality() {
    assert_runs_with_output(
        r#"
fn main()
    let r1 Result<int, bool> = Result.Ok(42)
    let r2 Result<int, bool> = Result.Ok(42)
    let r3 Result<int, bool> = Result.Ok(43)
    
    if r1 == r2
        println("Ok(42) == Ok(42)")
    if r1 != r3
        println("Ok(42) != Ok(43)")
"#,
        "Ok(42) == Ok(42)\nOk(42) != Ok(43)",
    );
}

#[test]
fn test_result_err_equality() {
    assert_runs_with_output(
        r#"
fn main()
    let r1 Result<int, bool> = Result.Err(true)
    let r2 Result<int, bool> = Result.Err(true)
    let r3 Result<int, bool> = Result.Err(false)
    
    if r1 == r2
        println("Err(true) == Err(true)")
    if r1 != r3
        println("Err(true) != Err(false)")
"#,
        "Err(true) == Err(true)\nErr(true) != Err(false)",
    );
}

#[test]
fn test_result_mixed_equality() {
    assert_runs_with_output(
        r#"
fn main()
    let r1 Result<int, bool> = Result.Ok(42)
    let r2 Result<int, bool> = Result.Err(true)
    
    if r1 != r2
        println("Ok(42) != Err(true)")
"#,
        "Ok(42) != Err(true)",
    );
}

#[test]
fn test_option_nested_equality() {
    assert_runs_with_output(
        r#"
fn main()
    let o1 Option<Option<int>> = Some(Some(3))
    let o2 Option<Option<int>> = Some(Some(3))
    let o3 Option<Option<int>> = Some(Some(4))
    
    if o1 == o2
        println("Some(Some(3)) == Some(Some(3))")
    if o1 != o3
        println("Some(Some(3)) != Some(Some(4))")
"#,
        "Some(Some(3)) == Some(Some(3))\nSome(Some(3)) != Some(Some(4))",
    );
}

#[test]
fn test_option_string_payload_equality() {
    assert_runs_with_output(
        r#"
fn s() String
    return "hel" + "lo"

fn main()
    let o1 Option<String> = Some(s())
    let o2 Option<String> = Some(s())
    
    if o1 == o2
        println("Some(s()) == Some(s()) true")
    else
        println("Some(s()) == Some(s()) false")
"#,
        "Some(s()) == Some(s()) true",
    );
}

#[test]
fn test_struct_scalar_fields_equality() {
    assert_runs_with_output(
        r#"
struct Point
    x int
    y int

fn main()
    let p1 = Point(1, 2)
    let p2 = Point(1, 2)
    let p3 = Point(1, 3)

    if p1 == p2
        println("Point(1,2) == Point(1,2)")
    if p1 != p3
        println("Point(1,2) != Point(1,3)")
"#,
        "Point(1,2) == Point(1,2)\nPoint(1,2) != Point(1,3)",
    );
}

#[test]
fn test_struct_string_field_equality() {
    assert_runs_with_output(
        r#"
fn s() String
    return "te" + "st"

struct Data
    name String
    value int

fn main()
    let d1 = Data(s(), 10)
    let d2 = Data(s(), 10)

    if d1 == d2
        println("Data(s(),10) == Data(s(),10) true")
    else
        println("Data(s(),10) == Data(s(),10) false")
"#,
        "Data(s(),10) == Data(s(),10) true",
    );
}

#[test]
fn test_tuple_equality() {
    assert_runs_with_output(
        r#"
fn main()
    let t1 (int, int) = (1, 2)
    let t2 (int, int) = (1, 2)
    let t3 (int, int) = (1, 3)
    
    if t1 == t2
        println("(1,2) == (1,2)")
    if t1 != t3
        println("(1,2) != (1,3)")
"#,
        "(1,2) == (1,2)\n(1,2) != (1,3)",
    );
}

#[test]
fn test_recursive_enum_rejected() {
    assert_compiler_error(
        r#"
enum Node
    Nil
    Cons(int, Node?)

fn main()
    let n = Node.Nil
    if n == Node.Nil
        println("ok")
"#,
        "Recursive type `Node` cannot use structural equality",
    );
}

#[test]
fn test_recursive_enum_with_equals_is_accepted() {
    // The refusal above tells the user to implement `equals`; this proves that
    // advice works, and that the method wins over the derived comparison.
    assert_runs_with_output(
        r#"
enum Node
    Nil
    Cons(int, Node?)

    public fn equals(other Self) bool
        return true

fn main()
    let n = Node.Nil
    if n == Node.Cons(1, None)
        println("own equals wins")
"#,
        "own equals wins",
    );
}

#[test]
fn test_mutually_recursive_enums_rejected() {
    // Neither enum contains itself directly; the cycle closes through the
    // other, which a guard that only compared against the outermost type
    // would miss.
    assert_compiler_error(
        r#"
enum Ping
    Stop
    ToPong(Pong?)

enum Pong
    Stop
    ToPing(Ping?)

fn main()
    let p = Ping.Stop
    if p == Ping.Stop
        println("ok")
"#,
        "Recursive type",
    );
}

#[test]
fn test_deep_acyclic_chain_rejected() {
    // A chain of distinct enums is not a cycle, so the cycle guard cannot
    // catch it; only the depth bound stops the inline expansion from
    // exhausting the compiler's stack.
    let depth = 80;
    let mut source = String::new();
    source.push_str("enum Level0\n    Leaf\n\n");
    for level in 1..depth {
        source.push_str(&format!(
            "enum Level{level}\n    Wrap(Level{prev}?)\n\n",
            level = level,
            prev = level - 1
        ));
    }
    source.push_str(&format!(
        "fn main()\n    let a = Level{last}.Wrap(None)\n    if a == Level{last}.Wrap(None)\n        println(\"ok\")\n",
        last = depth - 1
    ));
    assert_compiler_error(&source, "too deep");
}

#[test]
fn test_enum_with_equals_method() {
    assert_runs_with_output(
        r#"
enum Color
    Red
    Green
    Blue
    
    fn equals(other Self) bool
        return true

fn main()
    if Color.Red == Color.Green
        println("always equal via equals method")
"#,
        "always equal via equals method",
    );
}

#[test]
fn test_class_with_equals_dispatches_to_method() {
    assert_runs_with_output(
        r#"
use system.ops.{Equatable}

class Point implements Equatable
    var x int
    var y int
    public fn equals(other Self) bool
        return self.x == other.x and self.y == other.y

fn main()
    let p = Point(1, 2)
    let q = Point(1, 2)
    if p == q
        println("class equals dispatched")
"#,
        "class equals dispatched",
    );
}

#[test]
fn test_class_without_equals_keeps_reference_identity() {
    // A class carries behaviour, so its equality is its own to define. Without
    // an `equals` method `==` still answers "the same object", which is why
    // `assert_eq` refuses such a class rather than silently comparing
    // addresses inside a test.
    assert_runs_with_output(
        r#"
class Bare
    var x int

fn main()
    let a = Bare(1)
    let b = Bare(1)
    let alias = a
    if a != b
        println("distinct objects")
    if a == alias
        println("alias is the same object")
"#,
        "distinct objects\nalias is the same object",
    );
}

#[test]
fn test_sized_scalar_struct_fields_compare_at_their_own_width() {
    // Sized widths reach the comparison as struct fields and enum payloads.
    // A dispatch that handles only the default `int` and `float` turns these
    // into a compile error, and one that reads them at the base slot's width
    // compares the wrong bytes.
    assert_runs_with_output(
        r#"
struct Sized
    a i32
    b f32

fn main()
    let x Sized = Sized(7, 1.5)
    let same Sized = Sized(7, 1.5)
    let wider_int Sized = Sized(8, 1.5)
    let wider_float Sized = Sized(7, 2.5)

    if x == same
        println("equal")
    if x != wider_int
        println("i32 differs")
    if x != wider_float
        println("f32 differs")
"#,
        "equal\ni32 differs\nf32 differs",
    );
}

#[test]
fn test_multi_payload_variant_compares_every_payload() {
    assert_runs_with_output(
        r#"
fn s() String
    return "a" + "b"

enum Entry
    Pair(int, String)
    Empty

fn main()
    let a = Entry.Pair(1, s())
    let same = Entry.Pair(1, s())
    let other_int = Entry.Pair(2, s())

    if a == same
        println("both payloads equal")
    if a != other_int
        println("first payload differs")
    if a != Entry.Empty
        println("different variant")
"#,
        "both payloads equal\nfirst payload differs\ndifferent variant",
    );
}

#[test]
fn test_struct_field_holding_an_enum_compares_structurally() {
    assert_runs_with_output(
        r#"
enum Colour
    Red
    Blue

struct Tagged
    colour Colour
    count int

fn main()
    let a = Tagged(Colour.Red, 1)
    let same = Tagged(Colour.Red, 1)
    let other = Tagged(Colour.Blue, 1)

    if a == same
        println("nested enum equal")
    if a != other
        println("nested enum differs")
"#,
        "nested enum equal\nnested enum differs",
    );
}

#[test]
fn test_enum_payload_holding_a_struct_compares_structurally() {
    assert_runs_with_output(
        r#"
struct Point
    x int
    y int

enum Location
    At(Point)
    Nowhere

fn main()
    let a = Location.At(Point(5, 10))
    let same = Location.At(Point(5, 10))
    let other = Location.At(Point(5, 11))

    if a == same
        println("struct payload equal")
    if a != other
        println("struct payload differs")
    if a != Location.Nowhere
        println("payload variant differs from empty one")
"#,
        "struct payload equal\nstruct payload differs\npayload variant differs from empty one",
    );
}

#[test]
fn test_generic_enum_compares_its_substituted_payload() {
    // The payload type comes from substituting the enum's type argument, so a
    // managed and an unmanaged instantiation take different paths through the
    // comparison and both are checked here.
    assert_runs_with_output(
        r#"
fn s() String
    return "wrap" + "ped"

enum Box<T>
    Of(T)
    Nothing

fn main()
    let a Box<int> = Box.Of(7)
    let same Box<int> = Box.Of(7)
    let other Box<int> = Box.Of(8)
    if a == same
        println("int payload equal")
    if a != other
        println("int payload differs")

    let m Box<String> = Box.Of(s())
    let m_same Box<String> = Box.Of(s())
    if m == m_same
        println("managed payload equal")
"#,
        "int payload equal\nint payload differs\nmanaged payload equal",
    );
}
