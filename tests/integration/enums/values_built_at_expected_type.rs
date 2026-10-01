// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value a variant constructor builds binds only the type arguments its
//! payload names: `E.L(s)` is an `E<String, B>`. Stored where a concrete
//! `E<String, i128>` is declared — a parameter, a field, a return type, a
//! declared binding, a collection's elements, an assignment target — it is
//! built at that declared type, whose payload slots are wider, so a read or
//! release through the location finds the payload where it was written.

use super::super::utils::*;

const E: &str = r#"
use system.collections.list

enum E<A, B>
    L(A)
    R(B, A)

fn show(e E<String, i128>)
    match e
        E.R(n, s): println(f"r {s}")
        E.L(s): println(f"l {s}")
"#;

fn with_e(main: &str) -> String {
    format!("{E}\n{main}")
}

#[test]
fn a_call_argument_is_built_at_the_parameter_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    show(E.L("a" + ""))
"#,
        ),
        "l a",
    );
}

#[test]
fn a_named_call_argument_is_built_at_the_parameter_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    show(e: E.L("a" + ""))
"#,
        ),
        "l a",
    );
}

#[test]
fn a_method_argument_is_built_at_the_parameter_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
class Keeper
    fn keep(e E<String, i128>)
        show(e)

fn main()
    let k = Keeper()
    k.keep(E.L("a" + ""))
"#,
        ),
        "l a",
    );
}

#[test]
fn a_generic_class_constructor_argument_is_built_at_the_field_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
class Holder<T>
    var v T
    fn init(x T)
        self.v = x

fn main()
    let h = Holder<E<String, i128>>(E.L("a" + ""))
    show(h.v)
"#,
        ),
        "l a",
    );
}

#[test]
fn a_struct_field_initializer_is_built_at_the_field_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
struct Rec
    e E<String, i128>

fn main()
    let r = Rec(e: E.L("a" + ""))
    show(r.e)
"#,
        ),
        "l a",
    );
}

#[test]
fn a_returned_value_is_built_at_the_return_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn give() E<String, i128>
    return E.L("a" + "")

fn main()
    show(give())
"#,
        ),
        "l a",
    );
}

#[test]
fn a_declared_initializer_is_built_at_the_declared_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let e E<String, i128> = E.L("a" + "")
    show(e)
"#,
        ),
        "l a",
    );
}

#[test]
fn a_list_literal_is_built_at_the_declared_element_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let xs List<E<String, i128>> = List([E.L("a" + "")])
    for x in xs
        show(x)
"#,
        ),
        "l a",
    );
}

#[test]
fn a_pushed_value_is_built_at_the_element_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let xs = List<E<String, i128>>()
    xs.push(E.L("a" + ""))
    for x in xs
        show(x)
"#,
        ),
        "l a",
    );
}

#[test]
fn an_argument_in_a_match_arm_is_built_at_the_parameter_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let n = 0
    match n
        0: show(E.L("a" + ""))
        _: println("other")
"#,
        ),
        "l a",
    );
}

#[test]
fn an_assigned_value_is_built_at_the_target_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    var e E<String, i128> = E.R(5, "t" + "u")
    e = E.L("a" + "")
    show(e)
"#,
        ),
        "l a",
    );
}

#[test]
fn a_default_argument_is_built_at_the_parameter_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn show_default(e E<String, i128> = E.L("a" + ""))
    show(e)

fn main()
    show_default()
"#,
        ),
        "l a",
    );
}

#[test]
fn a_named_argument_binding_a_receivers_open_argument_refines_the_receiver() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn make() E<String, i128>
    return E.R(5, "t" + "u")

fn main()
    let xs = List([E.L("s" + "")])
    xs.push(item: make())
    for x in xs
        show(x)
"#,
        ),
        "l s\nr tu",
    );
}

#[test]
fn a_value_already_built_with_the_argument_open_is_refused_where_it_is_bound() {
    // `e` was built as an `E<String, B>`, at the narrower layout; it cannot be
    // rebuilt now, so passing it where an `E<String, i128>` is read is refused.
    assert_compiler_error(
        &with_e(
            r#"
fn main()
    let e = E.L("a" + "")
    show(e)
"#,
        ),
        "leaves `B` unbound",
    );
}

#[test]
fn a_tuple_element_is_built_at_the_declared_element_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let t (E<String, i128>, int) = (E.L("a" + ""), 1)
    show(t.0)
"#,
        ),
        "l a",
    );
}

#[test]
fn a_value_wrapped_into_a_declared_optional_is_built_at_the_payload_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    var o E<String, i128>? = E.L("a" + "")
    match o
        Some(e): show(e)
        None: println("none")
"#,
        ),
        "l a",
    );
}

#[test]
fn a_value_boxed_by_some_is_built_at_the_payload_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    var o Option<E<String, i128>> = Some(E.L("a" + ""))
    match o
        Some(e): show(e)
        None: println("none")
"#,
        ),
        "l a",
    );
}

#[test]
fn a_value_boxed_by_some_as_an_argument_is_built_at_the_payload_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn take(o E<String, i128>?)
    match o
        Some(e): show(e)
        None: println("none")

fn main()
    take(Some(E.L("a" + "")))
"#,
        ),
        "l a",
    );
}

#[test]
fn a_variant_payload_is_built_at_the_declared_payload_type() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let r Result<E<String, i128>, String> = Result.Ok(E.L("a" + ""))
    match r
        Result.Ok(e): show(e)
        Result.Err(m): println(m)
"#,
        ),
        "l a",
    );
}
