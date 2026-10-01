// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method is called on the value it is read from; read without a call it
//! has no receiver, so it is refused rather than run as a value.

use super::utils::*;

/// A method read without a call has no receiver bound to it, so it names no
/// function to run: it is refused, on an instance, on `self` inside a trait
/// default, and on an enum value alike.
#[test]
fn a_method_read_as_a_value_is_refused() {
    assert_compiler_error(
        r#"
class K
    n int
    fn a() int
        return self.n

fn main()
    let k = K(n: 4)
    let g = k.a
    println(f"{g()}")
"#,
        "MER_TYP_078",
    );
    assert_compiler_error(
        r#"
trait Tr
    fn a() int
    fn go() int
        let f = self.a
        return f()

class K implements Tr
    n int
    fn a() int
        return self.n

fn main()
    println(f"{K(n: 4).go()}")
"#,
        "the method `a` is read as a value, but a method can only be called",
    );
    assert_compiler_error(
        r#"
enum E
    One

    fn n() int
        return 1

fn main()
    let f = E.One.n
    println(f"{f()}")
"#,
        "MER_TYP_078",
    );
}

/// A field holding a function is a value, not a method, and a lambda
/// wrapping a method call hands the method on with its receiver.
#[test]
fn a_function_field_and_a_wrapped_method_are_values() {
    assert_runs_with_output(
        r#"
class K
    n int
    f fn() int
    fn a() int
        return self.n

fn main()
    let k = K(n: 4, f: fn() int: 9)
    let g = k.f
    let h = fn() int: k.a()
    println(f"{g()} {h()}")
"#,
        "9 4",
    );
}
