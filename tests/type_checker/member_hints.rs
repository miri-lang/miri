// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Member-not-found suggestions: the name a member borrowed from another
//! language maps to, the argument count that breaks a tie between two equally
//! close members, and the iteration hint that replaces an accessor a type does
//! not have.
//!
//! Every positive case pins the whole help line rather than the suggested name
//! alone. A substring passes when a longer member of the same synonym group is
//! suggested instead, which is how a wrong suggestion hides. Every negative
//! case asserts the help is *absent*, since an error message alone is reported
//! either way.

use super::utils::*;

#[test]
fn test_array_len_suggests_length() {
    type_checker_error_with_help_test(
        "
let l = [1, 2, 3]
let x = l.len()
",
        "Type 'Array' has no field or method 'len'",
        "Did you mean 'length'?",
    );
}

#[test]
fn test_list_len_suggests_length() {
    type_checker_error_with_help_test(
        "
var l [int] = [1, 2, 3]
l.len()
",
        "Type 'List' has no field or method 'len'",
        "Did you mean 'length'?",
    );
}

#[test]
fn test_list_append_suggests_push() {
    type_checker_error_with_help_test(
        "
var l [int] = [1, 2, 3]
l.append(2)
",
        "Type 'List' has no field or method 'append'",
        "Did you mean 'push'?",
    );
}

#[test]
fn test_string_upper_suggests_to_upper() {
    type_checker_error_with_help_test(
        "
let s = \"hello\"
s.upper()
",
        "Type 'String' has no field or method 'upper'",
        "Did you mean 'to_upper'?",
    );
}

/// `String` declares both `length()` and `size()`, so `len` has two members of
/// its synonym group to choose between. Edit distance decides, and it must land
/// on `length` — nothing in the compiler may assert which name is canonical.
#[test]
fn test_string_len_prefers_length_over_size() {
    type_checker_error_with_help_test(
        "
let s = \"hello\"
s.len()
",
        "Type 'String' has no field or method 'len'",
        "Did you mean 'length'?",
    );
}

#[test]
fn test_map_keys_suggests_iteration() {
    type_checker_error_with_help_test(
        "
let m {String: int} = {\"a\": 1}
m.keys()
",
        "Type 'Map' has no field or method 'keys'",
        "'Map' is iterable: use a 'for' loop over it instead of 'keys'.",
    );
}

/// The suggestion is read off the receiver's own members, so a class that names
/// its accessor `size` is served without the compiler knowing any stdlib name.
#[test]
fn test_user_class_size_matches_len() {
    type_checker_error_with_help_test(
        "
class Container
    fn size() int
        return 0

var c = Container()
c.len()
",
        "Type 'Container' has no field or method 'len'",
        "Did you mean 'size'?",
    );
}

/// A member reached through the base class is a candidate like any other.
#[test]
fn test_inherited_member_is_a_suggestion_candidate() {
    type_checker_error_with_help_test(
        "
class Base
    fn length() int
        return 0

class Derived extends Base
    fn other() int
        return 1

var d = Derived()
d.len()
",
        "Type 'Derived' has no field or method 'len'",
        "Did you mean 'length'?",
    );
}

#[test]
fn test_user_class_no_synonym_members() {
    type_checker_error_without_help_test(
        "
class Foo
    fn bar() int
        return 5

var f = Foo()
f.unknown()
",
        "Type 'Foo' has no field or method 'unknown'",
    );
}

#[test]
fn test_non_iterable_class_no_keys_help() {
    type_checker_error_without_help_test(
        "
class Foo
    fn bar() int
        return 5

var f = Foo()
f.keys()
",
        "Type 'Foo' has no field or method 'keys'",
    );
}

/// A type that declares the accessor itself resolves it — the iteration hint is
/// for a type that has no such member, not for every mention of the name.
#[test]
fn test_declared_keys_method_resolves() {
    type_checker_test(
        "
class Holder
    fn keys() int
        return 0

var h = Holder()
let n = h.keys()
",
    );
}

#[test]
fn test_arity_preference_zero_arg_call() {
    type_checker_error_with_help_test(
        "
class Container
    fn fooa() int
        return 0
    fn foob(x int) int
        return 0

var c = Container()
c.foo()
",
        "Type 'Container' has no field or method 'foo'",
        "Did you mean 'fooa'?",
    );
}

#[test]
fn test_arity_preference_one_arg_call() {
    type_checker_error_with_help_test(
        "
class Container
    fn fooa() int
        return 0
    fn foob(x int) int
        return 0

var c = Container()
c.foo(5)
",
        "Type 'Container' has no field or method 'foo'",
        "Did you mean 'foob'?",
    );
}

/// The receiver of a call is inferred after the arity has been consumed, so a
/// failed lookup inside it must not be ranked by the outer call's arity.
#[test]
fn test_arity_leak_nested_member_access() {
    type_checker_error_without_help_test(
        "
class Obj
    fn bar() Obj
        return Obj()
    fn len() int
        return 0

var o = Obj()
o.bar().unknown()
",
        "Type 'Obj' has no field or method 'unknown'",
    );
}

#[test]
fn test_arity_leak_field_access_call() {
    type_checker_error_without_help_test(
        "
fn f(x int)
    return

class Obj
    fn len() int
        return 0

var o = Obj()
f(o.unknown)
",
        "Type 'Obj' has no field or method 'unknown'",
    );
}
