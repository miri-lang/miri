// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method called on a value whose type is a trait-bounded parameter
//! (`x T` where `T extends Closable`) runs the implementing class's method, or
//! the trait's default where the class declares none — in a generic function,
//! in a generic caller handing the value on, and in a generic class holding it.

use super::utils::*;

const CLOSABLES: &str = r#"
trait Closable
    fn close() String
        return "default"

class Handle implements Closable
    public var id int

class Own implements Closable
    public var id int
    public fn close() String
        return "own"
"#;

#[test]
fn a_bounded_parameter_runs_the_class_method_or_the_trait_default() {
    let code = format!(
        "{CLOSABLES}
fn shut<T extends Closable>(x T) String
    return x.close()

fn twice<T extends Closable>(x T) String
    return shut(x) + shut(x)

fn main()
    println(shut(Handle(id: 1)))
    println(shut(Own(id: 2)))
    println(twice(Own(id: 3)))
"
    );
    assert_heap_guard_output(&code, "default\nown\nownown");
}

#[test]
fn a_generic_class_calls_the_method_its_bounded_field_answers() {
    let code = format!(
        "{CLOSABLES}
class Box<T extends Closable>
    v T
    fn init(v T)
        self.v = v
    public fn shut() String
        return self.v.close()

fn main()
    let a = Box<Handle>(Handle(id: 1))
    let b = Box<Own>(Own(id: 2))
    println(f\"{{a.shut()}} {{b.shut()}}\")
"
    );
    assert_heap_guard_output(&code, "default own");
}

/// The value handed to a bounded parameter is released where the body is
/// done with it, and its own drop hook runs exactly once.
#[test]
fn a_bounded_parameter_releases_the_value_through_its_drop_hook_once() {
    assert_heap_guard_output(
        r#"
trait Closable
    fn close() String

class Res implements Closable
    public var id int
    public fn close() String
        return "closed"
    fn drop()
        println(f"drop {self.id}")

fn finish<T extends Closable>(x T)
    println(x.close())

fn main()
    finish(Res(id: 7))
    println("end")
"#,
        "closed\ndrop 7\nend",
    );
}
