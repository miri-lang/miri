// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What an inherited method sees is decided by the `extends` clause, not by the
//! type arguments the child itself carries. `class Child extends Base<String>`
//! carries none and still reaches the parent's bodies compiled for `String`;
//! `class Wrap<U> extends Base<[U]>` reaches them compiled for a list. Without
//! the clause a call falls back to the shared body a generic class leaves
//! behind, which reads every parameter as an untyped word.
//!
//! Strings are built at run time so an empty answer or an unretained reference
//! cannot pass for the right one.

use super::utils::*;

const PINNED: &str = r#"
class Base<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v

class Child extends Base<String>
    fn init(v String)
        super.init(v)
"#;

fn with_pinned(main: &str) -> String {
    format!("{PINNED}\n{main}")
}

#[test]
fn an_inherited_method_returns_the_value_the_extends_clause_pins() {
    assert_runs_with_output(
        &with_pinned(
            r#"
fn main()
    let c = Child("C".to_lower())
    println(c.get())
"#,
        ),
        "c",
    );
}

#[test]
fn an_inherited_method_on_a_temporary_returns_its_value() {
    assert_runs_with_output(
        &with_pinned(
            r#"
fn main()
    println(Child("C".to_lower()).get())
    println("after")
"#,
        ),
        "c\nafter",
    );
}

#[test]
fn an_inherited_method_follows_a_clause_that_wraps_the_childs_parameter() {
    // `Base<[U]>` reaches the parent at a list of the child's parameter, so the
    // inherited `get` returns a list rather than the element the child carries.
    assert_runs_with_output(
        r#"
use system.collections.list

class Base<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v

class Wrap<U> extends Base<[U]>
    fn init(v [U])
        super.init(v)

fn main()
    let w = Wrap<String>(List(["W".to_lower()]))
    println(w.get()[0])
"#,
        "w",
    );
}

#[test]
fn an_inherited_method_follows_a_clause_that_fills_one_of_several_parameters() {
    // The child carries two parameters and the parent one; only the clause says
    // which of the child's fills it, so matching the parent's parameter by name
    // finds nothing.
    assert_runs_with_output(
        r#"
class Base<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v

class Swap<A, B> extends Base<B>
    fn init(v B)
        super.init(v)

fn main()
    let s = Swap<int, String>("S".to_lower())
    println(s.get())
"#,
        "s",
    );
}

#[test]
fn an_inherited_method_returns_an_integer_the_extends_clause_pins() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v

class Counter extends Box<int>
    fn init(v int)
        super.init(v)

fn main()
    let c = Counter(7)
    println(f"{c.get()}")
"#,
        "7",
    );
}
