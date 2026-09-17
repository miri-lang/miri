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

const SELF_TAGGED: &str = r#"
use system.collections.set

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Self) bool
        return self.value == other.value

    public fn me() Self
        let copy Self = Tagged<T>(self.value)
        return copy
"#;

fn with_self_tagged(main: &str) -> String {
    format!("{SELF_TAGGED}\n{main}")
}

#[test]
fn direct_equals_call_with_a_self_parameter_compares_by_content() {
    assert_runs_with_output(
        &with_self_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let u = Tagged<String>("PEAR".to_lower())
    let v = Tagged<String>("FIG".to_lower())
    println(f"{t.equals(u)},{t.equals(v)}")
"#,
        ),
        "true,false",
    );
}

#[test]
fn self_return_type_is_the_class_at_its_instantiation() {
    assert_runs_with_output(
        &with_self_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let m Tagged<String> = t.me()
    println(f"{m.value},{t.equals(m)},{m == t}")
"#,
        ),
        "pear,true,true",
    );
}

#[test]
fn set_contains_agrees_with_a_direct_self_equals_call() {
    assert_runs_with_output(
        &with_self_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let probe = Tagged<String>("PEAR".to_lower())
    var s = Set<Tagged<String>>()
    s.add(t)
    println(f"{s.contains(probe)},{t.equals(probe)}")
"#,
        ),
        "true,true",
    );
}

#[test]
fn self_nested_in_option_and_list_resolves_on_a_two_parameter_class() {
    assert_runs_with_output(
        r#"
use system.ops
use system.collections.map
use system.collections.list

class Pair<K, V> implements Equatable
    key K
    val V

    fn init(key K, val V)
        self.key = key
        self.val = val

    public fn equals(other Self) bool
        return self.key == other.key and self.val == other.val

    public fn copy() Self
        return Pair<K, V>(self.key, self.val)

    public fn maybe(flag bool) Self?
        if flag
            return self.copy()
        return None

    public fn twice() List<Self>
        var out = List<Self>()
        out.push(self.copy())
        out.push(self.copy())
        return out

fn main()
    let a = Pair<String, int>("k".to_lower(), 1)
    let b = Pair<String, int>("K".to_lower(), 1)
    let c = Pair<String, int>("k".to_lower(), 2)
    println(f"{a.equals(b)},{a.equals(c)},{a == b},{a != c}")
    match a.maybe(true)
        Some(p): println(f"some {p.equals(b)}")
        None: println("none")
    let l = a.twice()
    println(f"{l.length()} {l[1].equals(b)}")
    var mp = Map<Pair<String, int>, int>()
    mp.set(a, 7)
    println(f"{mp.contains_key(b)},{mp.contains_key(c)}")
"#,
        "true,false,true,true\nsome true\n2 true\ntrue,false",
    );
}

#[test]
fn self_parameter_refuses_a_different_instantiation() {
    assert_compiler_error(
        &with_self_tagged(
            r#"
fn main()
    let t = Tagged<String>("PEAR".to_lower())
    let n = Tagged<int>(1)
    println(f"{t.equals(n)}")
"#,
        ),
        "expected Tagged<String>, got Tagged<int>",
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
