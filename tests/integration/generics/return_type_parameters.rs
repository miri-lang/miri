// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic function whose type parameter appears only in its return type —
//! `fn make<T>() Box<T>` — takes the parameter from the type of the location
//! its result goes into: a declared binding, a parameter, a return type. The
//! body is compiled at that instantiation, so a managed value it holds is laid
//! out and released as that type. Where nothing names the type, the call is
//! refused at the call rather than compiled against the bare parameter.

use super::utils::*;

const BOX: &str = r#"
class Box<T>
    v T?
    fn put(a T) T
        self.v = a
        return a

fn make<T>() Box<T>
    return Box<T>()
"#;

const OP: &str = r#"
trait Op<T>
    fn keep(a T) T

class Impl<T> implements Op<T>
    fn keep(a T) T
        return a

fn make<T>() Op<T>
    return Impl<T>()
"#;

#[test]
fn a_declared_binding_pins_a_returned_class() {
    let code = format!(
        "{BOX}
fn main()
    let b Box<String> = make()
    println(b.put(\"a\" + \"b\"))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_declared_binding_pins_a_returned_trait() {
    let code = format!(
        "{OP}
fn main()
    let o Op<String> = make()
    println(o.keep(\"a\" + \"b\"))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_declared_binding_agrees_with_an_explicit_type_argument() {
    let code = format!(
        "{OP}
fn main()
    let o Op<String> = make<String>()
    println(o.keep(\"a\" + \"b\"))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_parameter_pins_the_call_passed_to_it() {
    let code = format!(
        "{BOX}
fn take(b Box<String>) String
    return b.put(\"a\" + \"b\")

fn main()
    println(take(make()))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_return_type_pins_the_call_returned() {
    let code = format!(
        "{BOX}
fn build() Box<String>
    return make()

fn main()
    let b = build()
    println(b.put(\"a\" + \"b\"))
"
    );
    assert_heap_guard_output(&code, "ab");
}

/// The calling body declares a parameter of the same name as the callee's:
/// the binding's type, not the caller's parameter, decides the instantiation.
#[test]
fn a_callers_parameter_of_the_same_name_does_not_pin_the_call() {
    let code = format!(
        "{BOX}
fn outer<T>(x T) String
    let b Box<String> = make()
    return b.put(\"a\" + \"b\")

fn main()
    println(outer(5))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_declared_optional_binding_pins_the_call_it_wraps() {
    let code = format!(
        "{BOX}
fn main()
    let b Box<String>? = make()
    match b
        Some(inner): println(inner.put(\"a\" + \"b\"))
        None: println(\"none\")
"
    );
    assert_heap_guard_output(&code, "ab");
}

/// The refusal of `code` is exactly one error, naming `expected`.
fn assert_refused_once(code: &str, expected: &str) {
    let result = crate::utils::miri_check(code);
    let output = result.output();
    assert!(
        output.contains(expected),
        "expected the refusal '{expected}':\n{output}"
    );
    assert_eq!(
        output.matches("error[").count(),
        1,
        "the refused call must be the one error reported:\n{output}"
    );
}

const PASS2: &str = r#"
fn pass2<U>(b Box<U>, y U) Box<U>
    b.put(y)
    return b
"#;

/// An open call handed to another generic call is typed at its own slot,
/// never at a parameter of the calling body that shares its name.
#[test]
fn an_open_call_handed_to_a_generic_call_is_refused_once() {
    let code = format!(
        "{BOX}{PASS2}
fn outer<T>(x T) T
    let b = pass2(make(), 5)
    return b.put(x)

fn main()
    println(outer(\"a\" + \"b\"))
"
    );
    assert_refused_once(&code, "Cannot infer type argument `T` of `make`");
}

#[test]
fn renaming_the_callers_parameter_does_not_change_the_refusal() {
    let code = format!(
        "{BOX}{PASS2}
fn outer<V>(x V) V
    let b = pass2(make(), 5)
    return b.put(x)

fn main()
    println(outer(\"a\" + \"b\"))
"
    );
    assert_refused_once(&code, "Cannot infer type argument `T` of `make`");
}

#[test]
fn an_open_call_handed_to_a_callee_sharing_its_parameter_name_is_refused_once() {
    let code = format!(
        "{BOX}
fn pass2<T>(b Box<T>, y T) Box<T>
    b.put(y)
    return b

fn outer<T>(x T) T
    let b = pass2(make(), 5)
    return b.put(x)

fn main()
    println(outer(\"a\" + \"b\"))
"
    );
    assert_refused_once(&code, "Cannot infer type argument `T` of `make`");
}

#[test]
fn an_open_call_formatted_into_a_string_is_refused_once() {
    let code = format!(
        "{BOX}
fn main()
    println(f\"{{make()}}\")
"
    );
    assert_refused_once(&code, "Cannot infer type argument `T` of `make`");
}

#[test]
fn a_method_called_on_an_open_call_is_refused_once() {
    let code = format!(
        "{BOX}
fn main()
    let n int = make().put(1)
    println(f\"{{n}}\")
"
    );
    assert_refused_once(&code, "Cannot infer type argument `T` of `make`");
}

#[test]
fn an_open_call_in_a_default_argument_is_refused_once() {
    let code = format!(
        "{BOX}
fn g(n int = make().put(1)) int
    return n

fn main()
    println(f\"{{g()}}\")
"
    );
    assert_refused_once(&code, "Cannot infer type argument `T` of `make`");
}

/// An open call stored where no binding of its parameters can fit reports
/// that mismatch, not a missing type argument.
#[test]
fn an_open_call_passed_where_no_binding_fits_reports_the_mismatch() {
    let code = format!(
        "{BOX}
fn takes(x int) int
    return x

fn main()
    let q = takes(make())
    println(f\"{{q}}\")
"
    );
    assert_refused_once(
        &code,
        "Type mismatch for argument 'x': expected int, got Box<T>",
    );
}

#[test]
fn an_open_call_handed_to_a_method_where_no_binding_fits_reports_the_mismatch() {
    let code = format!(
        "{BOX}
fn main()
    let b Box<String> = make()
    b.put(make())
    println(\"x\")
"
    );
    assert_refused_once(
        &code,
        "Type mismatch for argument 'a': expected String, got Box<T>",
    );
}

/// Of two parameters the argument binds one; the refusal names only the other.
#[test]
fn a_refusal_names_only_the_parameters_left_unbound() {
    assert_refused_once(
        r#"
class Pair<A, B>
    first A?
    second B?

fn make<A, B>(a A) Pair<A, B>
    var p = Pair<A, B>()
    p.first = a
    return p

fn main()
    let p = make(5)
    println("unreachable")
"#,
        "Cannot infer type argument `B` of `make`",
    );
}

/// A parameter neither an argument nor the return type mentions cannot be
/// bound by the location either, so the refusal does not suggest declaring it.
#[test]
fn a_parameter_the_signature_never_mentions_must_be_written_out() {
    let code = r#"
fn noop<T>(x int) int
    return x

fn main()
    let r int = noop(5)
    println(f"{r}")
"#;
    assert_refused_once(
        code,
        "Cannot infer type argument `T` of `noop`: neither an argument nor the return type mentions it",
    );
    let output = crate::utils::miri_check(code).output();
    assert!(
        !output.contains("declare the type of the location"),
        "a location cannot bind a parameter the return type never names:\n{output}"
    );
}

/// A mismatch found once the location has bound the call reports the type
/// the call was bound at.
#[test]
fn a_mismatch_after_binding_reports_the_bound_type() {
    assert_compiler_error(
        r#"
class Pair<A, B>
    first A?
    second B?

fn mk<A, B>(a A) Pair<A, B>
    return Pair<A, B>()

fn main()
    let p Pair<int, int> = mk("s")
    println("unreachable")
"#,
        "expected Pair<int, int>, got Pair<String, int>",
    );
}

#[test]
fn a_list_constructor_pins_the_calls_in_its_literal() {
    let code = format!(
        "{BOX}
use system.collections.list

fn main()
    let xs = List<Box<String>>([make()])
    println(xs[0].put(\"a\" + \"b\"))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_list_constructor_passed_as_an_argument_pins_the_calls_in_its_literal() {
    let code = format!(
        "{BOX}
use system.collections.list

fn take(xs List<Box<String>>) String
    return xs[0].put(\"a\" + \"b\")

fn main()
    println(take(List<Box<String>>([make()])))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn an_array_constructor_pins_the_calls_it_is_handed() {
    let code = format!(
        "{BOX}
use system.collections.array

fn main()
    let xs = Array<Box<String>, 1>(make())
    println(xs[0].put(\"a\" + \"b\"))
"
    );
    assert_heap_guard_output(&code, "ab");
}

#[test]
fn a_set_constructor_pins_the_calls_in_its_literal() {
    let code = format!(
        "{BOX}
use system.collections.set

fn main()
    let xs = Set<Box<String>>({{make()}})
    println(f\"{{xs.length()}}\")
"
    );
    assert_heap_guard_output(&code, "1");
}

/// Nothing names the type: the call is refused where it is written, and the
/// binding it initializes is not then reported again at every use.
#[test]
fn a_call_nothing_pins_is_refused_once_at_the_call() {
    let code = format!(
        "{BOX}
fn main()
    let b = make()
    println(b.put(\"a\" + \"b\"))
"
    );
    let result = crate::utils::miri_check(&code);
    let output = result.output();
    assert!(
        output.contains("MER_TYP_048")
            && output.contains("Cannot infer type argument `T` of `make`"),
        "expected the call to be refused for its unbound parameter:\n{output}"
    );
    assert_eq!(
        output.matches("error[").count(),
        1,
        "the refused call must not be reported again at its uses:\n{output}"
    );
}

#[test]
fn a_call_nothing_pins_as_a_statement_is_refused() {
    let code = format!(
        "{BOX}
fn main()
    make()
"
    );
    assert_compiler_error(&code, "Cannot infer type argument `T` of `make`");
}

/// The body orders its parameter's values, and only the binding's type pins
/// the parameter — to a type with no ordering, which is refused.
#[test]
fn a_parameter_pinned_by_the_binding_is_held_to_the_bodys_requirements() {
    assert_compiler_error(
        r#"
use system.collections.list

struct P
    v int

fn least<T>() T?
    let xs = List<T>()
    return xs.min()

fn main()
    let p P? = least()
    println("unreachable")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn a_parameter_pinned_by_the_binding_to_an_ordered_type_runs() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn least<T>() T?
    let xs = List<T>()
    return xs.min()

fn main()
    let p String? = least()
    match p
        Some(s): println(s)
        None: println("none")
"#,
        "none",
    );
}
