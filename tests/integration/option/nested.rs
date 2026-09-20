// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_returning_an_optional_as_an_optional_of_optional_wraps_it() {
    assert_heap_guard_output(
        r#"
fn lift(o int?) Option<int?>
    return o

fn main()
    match lift(Some(9))
        Some(inner): println(f"inner {inner}")
        None: println("outer none")
"#,
        "inner Some(9)",
    );
}

#[test]
fn test_lifting_none_is_some_none_and_a_none_literal_is_the_outer_none() {
    assert_heap_guard_output(
        r#"
fn lift(o int?) Option<int?>
    return o

fn nothing() Option<int?>
    return None

fn show(o Option<int?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    show(lift(None))
    show(nothing())
    show(lift(Some(4)))
"#,
        "some none\nnone\nsome some 4",
    );
}

#[test]
fn test_an_optional_initializer_for_an_optional_of_optional_is_wrapped() {
    assert_heap_guard_output(
        r#"
fn main()
    let inner int? = None
    let o Option<int?> = inner
    match o
        Some(i): println(f"some {i}")
        None: println("none")
"#,
        "some None",
    );
}

#[test]
fn test_first_and_last_over_a_list_of_optional_ints() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn show(o Option<int?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    var xs = List<int?>()
    show(xs.first())
    xs.push(Some(9))
    xs.push(None)
    show(xs.first())
    show(xs.last())
    let f = xs.first()
    println(f"{f}")
"#,
        "none\nsome some 9\nsome none\nSome(Some(9))",
    );
}

#[test]
fn test_first_and_last_over_a_list_of_optional_strings() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn show(o Option<String?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    var xs = List<String?>()
    show(xs.last())
    xs.push(None)
    xs.push(Some("tail"))
    show(xs.first())
    show(xs.last())
"#,
        "none\nsome none\nsome some tail",
    );
}

#[test]
fn test_a_bare_value_returned_or_passed_where_option_is_spelled_is_boxed() {
    assert_heap_guard_output(
        r#"
fn five() Option<int>
    return 5

fn take(o Option<int>)
    match o
        Some(v): println(f"take {v}")
        None: println("take none")

fn main()
    match five()
        Some(v): println(f"five {v}")
        None: println("none")
    take(7)
    take(None)
"#,
        "five 5\ntake 7\ntake none",
    );
}

#[test]
fn test_a_lifted_optional_passed_straight_to_a_call_is_released_once() {
    assert_heap_guard_output(
        r#"
fn lift(o int?) Option<int?>
    return o

fn make() Option<String?>
    return Some(Some("made"))

fn peek(o Option<int?>)
    match o
        Some(inner): println(f"peek {inner}")
        None: println("peek none")

fn show(o Option<String?>)
    match o
        Some(inner): println(f"show {inner}")
        None: println("show none")

fn main()
    peek(lift(Some(1)))
    show(make())
"#,
        "peek Some(1)\nshow Some(made)",
    );
}

#[test]
fn test_a_bare_value_two_optional_layers_short_is_boxed_for_every_layer() {
    assert_heap_guard_output(
        r#"
fn main()
    let o Option<int?> = 5
    match o
        Some(mid): println(f"{mid}")
        None: println("none")
"#,
        "Some(5)",
    );
}

#[test]
fn test_a_managed_optional_two_layers_short_is_boxed_for_every_layer() {
    assert_heap_guard_output(
        r#"
fn main()
    let inner String? = Some("deep")
    let o Option<Option<String?>> = inner
    match o
        Some(mid): match mid
            Some(i): println(f"deep {i}")
            None: println("mid none")
        None: println("none")
"#,
        "deep Some(deep)",
    );
}

#[test]
fn test_a_none_literal_into_a_two_layer_target_stays_the_outer_none() {
    assert_heap_guard_output(
        r#"
fn nothing() Option<int?>
    return None

fn main()
    let o Option<int?> = None
    match o
        Some(mid): println(f"some {mid}")
        None: println("none")
    match nothing()
        Some(mid): println(f"some {mid}")
        None: println("returned none")
"#,
        "none\nreturned none",
    );
}

#[test]
fn test_a_value_two_optional_layers_short_is_boxed_at_every_coercion_site() {
    assert_heap_guard_output(
        r#"
class Box
    var held Option<String?>

fn lift() Option<Option<int?>>
    return 3

fn take(o Option<String?>)
    match o
        Some(mid): println(f"take {mid}")
        None: println("take none")

fn main()
    take("arg")
    let b = Box("field")
    match b.held
        Some(mid): println(f"field {mid}")
        None: println("field none")
    var assigned Option<String?> = None
    assigned = "assigned"
    match assigned
        Some(mid): println(f"assign {mid}")
        None: println("assign none")
    match lift()
        Some(mid): println(f"ret {mid}")
        None: println("ret none")
"#,
        "take Some(arg)\nfield Some(field)\nassign Some(assigned)\nret Some(Some(3))",
    );
}

/// A double release shows up as corruption only once enough allocations have
/// cycled, so the source is boxed forty times rather than once.
#[test]
fn test_boxing_through_two_layers_leaves_an_owned_source_to_its_owner() {
    let spins = "take Some(spin)\n".repeat(40);
    let expected = format!("take Some(owned)\ntake Some(owned)\nstill owned\n{spins}spun");
    assert_heap_guard_output(
        r#"
fn take(o Option<String?>)
    match o
        Some(mid): println(f"take {mid}")
        None: println("take none")

fn main()
    let owned = "owned"
    take(owned)
    take(owned)
    println(f"still {owned}")
    var i = 0
    while i < 40
        take("spin")
        i = i + 1
    println("spun")
"#,
        &expected,
    );
}

#[test]
fn test_nested_optionals_of_strings_are_heap_guard_clean() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn lift(o String?) Option<String?>
    return o

fn show(o Option<String?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    var xs = List<String?>()
    xs.push(Some("head"))
    xs.push(None)
    show(xs.first())
    show(xs.last())
    show(lift(Some("lifted")))
    show(lift(None))
"#,
        "some some head\nsome none\nsome some lifted\nsome none",
    );
}

#[test]
fn test_a_bare_value_passed_to_a_constructor_is_boxed_for_the_init_parameter() {
    // A class with a user-defined `init` received its arguments exactly as they
    // were lowered, without being compared against the parameter declared. A
    // bare value passed where an optional is written reached the body unboxed,
    // and the first read took the payload for the address of an optional.
    assert_runs_with_output(
        r#"
class Box
    var held String?
    fn init(held String?)
        self.held = held

fn main()
    let b = Box("field")
    match b.held
        Some(mid): println(f"field {mid}")
        None: println("field none")
"#,
        "field field",
    );
}

#[test]
fn test_a_constructor_argument_two_optional_layers_short_is_boxed_for_every_layer() {
    assert_runs_with_output(
        r#"
class Box
    var held Option<String?>
    fn init(held Option<String?>)
        self.held = held

fn main()
    let b = Box("field")
    match b.held
        Some(inner)
            match inner
                Some(mid): println(f"two {mid}")
                None: println("two inner none")
        None: println("two none")
"#,
        "two field",
    );
}

#[test]
fn test_a_none_literal_constructor_argument_stays_the_outer_none() {
    // Passes before the coercion existed, because a `None` literal already
    // lowered as the outer absence. It guards the coercion against lifting it
    // into `Some(None)`.
    assert_runs_with_output(
        r#"
class Box
    var held Option<String?>
    fn init(held Option<String?>)
        self.held = held

fn main()
    let b = Box(None)
    match b.held
        Some(inner): println("outer some")
        None: println("outer none")
"#,
        "outer none",
    );
}

#[test]
fn test_a_constructor_argument_already_at_the_declared_depth_is_not_reboxed() {
    // Also passes beforehand — nothing was coerced at all. It is here to catch
    // the coercion boxing a value that already matched.
    // An argument that already matches the parameter must pass through
    // untouched; boxing it again would put the value one layer too deep.
    assert_runs_with_output(
        r#"
class Box
    var held String?
    fn init(held String?)
        self.held = held

fn main()
    let ready String? = Some("ready")
    let b = Box(ready)
    match b.held
        Some(mid): println(f"depth {mid}")
        None: println("depth none")
"#,
        "depth ready",
    );
}

#[test]
fn test_a_named_constructor_argument_is_boxed_for_its_init_parameter() {
    // The named form reaches the same loop, and the parameter it names is what
    // decides the depth its value is boxed to.
    assert_runs_with_output(
        r#"
class Pair
    var first String?
    var second int
    fn init(first String?, second int)
        self.first = first
        self.second = second

fn main()
    let p = Pair(first: "named", second: 7)
    match p.first
        Some(mid): println(f"named {mid} {p.second}")
        None: println("named none")
"#,
        "named named 7",
    );
}

#[test]
#[ignore = "a named argument is passed in the position it was written rather than the position it names, so arguments given out of declaration order are silently swapped. Independent of optionals and of the boxing this file otherwise covers: two plain int parameters reproduce it, and a plain function call does too, while a struct literal and a class without an init are both correct because they match arguments to fields by name"]
fn test_a_named_constructor_argument_out_of_declaration_order_binds_by_name() {
    assert_runs_with_output(
        r#"
class Pair
    var first String?
    var second int
    fn init(first String?, second int)
        self.first = first
        self.second = second

fn main()
    let p = Pair(second: 7, first: "named")
    match p.first
        Some(mid): println(f"named {mid} {p.second}")
        None: println("named none")
"#,
        "named named 7",
    );
}
