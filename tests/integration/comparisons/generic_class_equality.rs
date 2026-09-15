// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `==`, `!=` and the ordering operators on instances of a generic class call
//! the method body compiled for the instantiation, so a managed type argument
//! is compared by content rather than by address.

use super::utils::*;

const TAGGED: &str = r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Tagged<T>) bool
        return self.value == other.value
"#;

fn with_tagged(main: &str) -> String {
    format!("{TAGGED}\n{main}")
}

fn with_set_and_tagged(main: &str) -> String {
    format!("use system.collections.set\n{TAGGED}\n{main}")
}

#[test]
fn equal_operator_compares_a_managed_type_argument_by_content() {
    assert_runs_with_output(
        &with_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let u = Tagged<String>("PEAR".to_lower())
    let e = t == u
    println(f"eq={e}")
"#,
        ),
        "eq=true",
    );
}

#[test]
fn not_equal_operator_negates_the_instantiated_equals() {
    assert_runs_with_output(
        &with_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let u = Tagged<String>("PEAR".to_lower())
    let v = Tagged<String>("FIG".to_lower())
    println(f"{t != u},{t != v},{t == v}")
"#,
        ),
        "false,true,false",
    );
}

#[test]
fn equal_operator_on_an_int_instantiation_compares_by_value() {
    assert_runs_with_output(
        &with_tagged(
            r#"
fn main()
    let a = Tagged<int>(40 + 2)
    let b = Tagged<int>(42)
    let c = Tagged<int>(7)
    println(f"{a == b},{a == c},{a != c}")
"#,
        ),
        "true,false,true",
    );
}

#[test]
fn equal_operator_and_set_contains_agree_on_a_generic_class() {
    assert_runs_with_output(
        &with_set_and_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let probe = Tagged<String>("PEAR".to_lower())
    var s = Set<Tagged<String>>()
    s.add(t)
    println(f"{s.contains(probe)},{t == probe}")
"#,
        ),
        "true,true",
    );
}

#[test]
fn equal_operator_with_a_self_parameter_compares_by_content() {
    assert_runs_with_output(
        r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Self) bool
        return self.value == other.value

fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let u = Tagged<String>("PEAR".to_lower())
    println(f"eq={t == u}")
"#,
        "eq=true",
    );
}

#[test]
fn equal_operator_inside_a_generic_function_compares_by_content() {
    assert_runs_with_output(
        &with_tagged(
            r#"
fn same<T>(a T, b T) bool
    let x = Tagged<T>(a)
    let y = Tagged<T>(b)
    return x == y

fn main()
    let equal = same("PEAR".to_lower(), "PEAR".to_lower())
    let differ = same("A".to_lower(), "B".to_lower())
    println(f"{equal},{differ}")
"#,
        ),
        "true,false",
    );
}

#[test]
fn equal_operator_on_optional_generic_class_values_compares_by_content() {
    assert_runs_with_output(
        &with_tagged(
            r#"
fn main()
    var t Tagged<String>? = None
    var u Tagged<String>? = None
    t = Tagged<String>("PEAR".to_lower())
    u = Tagged<String>("PEAR".to_lower())
    let v Tagged<String>? = None
    println(f"{t == u},{t == v},{t != u}")
"#,
        ),
        "true,false,false",
    );
}

#[test]
fn equal_operator_on_enum_payloads_of_a_generic_class_compares_by_content() {
    assert_runs_with_output(
        &with_tagged(
            r#"
enum Holder
    Empty
    Full(Tagged<String>)

fn main()
    let a = Holder.Full(Tagged<String>("PEAR".to_lower()))
    let b = Holder.Full(Tagged<String>("PEAR".to_lower()))
    let c = Holder.Full(Tagged<String>("FIG".to_lower()))
    println(f"{a == b},{a == c}")
"#,
        ),
        "true,false",
    );
}

#[test]
fn ordering_operator_calls_the_instantiated_compare() {
    assert_runs_with_output(
        r#"
use system.ops

class Tagged<T> implements Comparable
    value T

    fn init(value T)
        self.value = value

    public fn compare(other Self) int
        if self.value == other.value
            return 0
        return 1

fn main()
    let a = Tagged<String>("PEAR".to_lower())
    let b = Tagged<String>("PEAR".to_lower())
    println(f"{a <= b},{a >= b},{a < b}")
"#,
        "true,true,false",
    );
}
