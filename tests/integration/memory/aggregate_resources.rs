// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A tuple or an array holding a resource is a resource.
//!
//! A resource is any type whose release may run a `drop` hook, so a value
//! holding one is one too, decided by its type alone. Assigning it to a
//! second binding moves it, a later use of the first binding is refused with
//! MER_OWN_003, and the hook runs exactly once. A tuple or array of plain
//! values is no resource and is still copied.

use super::super::utils::*;

const RESOURCE: &str = r#"
class Res
    public var n int
    fn init(n int)
        self.n = n
    fn drop(self)
        println(f"drop {self.n}")
"#;

/// `main` declaring `a` as `declaration`, assigning it to `b`, then running
/// `after`, then reading `b` through `read`.
fn moved(declaration: &str, after: &str, read: &str) -> String {
    format!(
        "{RESOURCE}\nfn main()\n    let a {declaration}\n    let b = a\n{after}    println(f\"{{{read}}} end\")\n"
    )
}

/// Asserts `source` is refused with MER_OWN_003 at the read of `a` after it
/// moved to `b`, with the exact message.
fn assert_use_after_move_refused(source: &str) {
    let diagnostic = assert_refused_at(source, "MER_OWN_003", "println(f\"{a.");
    assert_eq!(
        diagnostic.message,
        "'a' was consumed by 'assignment to 'b'' and cannot be used again\n  fix: use 'a' \
         before the call that keeps it, or pass it to a function that only reads it"
    );
}

/// The line reading `a` after it moved.
const READ_LENGTH: &str = "    println(f\"{a.length()}\")\n";

#[test]
fn reading_an_array_of_a_resource_after_it_moved_is_refused() {
    assert_use_after_move_refused(&moved("= [Res(1)]", READ_LENGTH, "b.length()"));
}

#[test]
fn reading_an_array_of_a_resource_spelled_in_full_after_it_moved_is_refused() {
    assert_use_after_move_refused(&moved(
        "Array<Res, 1> = [Res(1)]",
        READ_LENGTH,
        "b.length()",
    ));
}

#[test]
fn reading_a_tuple_of_a_resource_after_it_moved_is_refused() {
    assert_use_after_move_refused(&moved("= (Res(1), 2)", "    println(f\"{a.1}\")\n", "b.1"));
}

#[test]
fn reading_a_tuple_nesting_an_array_of_a_resource_after_it_moved_is_refused() {
    assert_use_after_move_refused(&moved(
        "= (2, [Res(1)])",
        "    println(f\"{a.0}\")\n",
        "b.0",
    ));
}

#[test]
fn an_array_of_a_resource_moved_runs_its_hook_once() {
    assert_heap_guard_output(&moved("= [Res(1)]", "", "b.length()"), "1 end\ndrop 1");
}

#[test]
fn a_tuple_of_a_resource_moved_runs_its_hook_once() {
    assert_heap_guard_output(&moved("= (Res(1), 2)", "", "b.1"), "2 end\ndrop 1");
}

#[test]
fn a_tuple_of_plain_values_is_still_copied() {
    assert_heap_guard_output(
        "fn main()\n    let a = (1, \"x\")\n    let b = a\n    println(f\"{a.0} {b.1}\")\n",
        "1 x",
    );
}

#[test]
fn an_array_of_plain_values_is_still_copied() {
    assert_heap_guard_output(
        "fn main()\n    let a = [1, 2]\n    let b = a\n    println(f\"{a.length()} {b.length()}\")\n",
        "2 2",
    );
}

#[test]
fn a_tuple_of_a_class_without_a_hook_is_still_copied() {
    assert_heap_guard_output(
        "class P\n    public var n int\n    fn init(n int)\n        self.n = n\n\nfn main()\n    let a = (P(1), 2)\n    let b = a\n    println(f\"{a.1} {b.1}\")\n",
        "2 2",
    );
}

/// `main` declaring `a` as `declaration` and ending its scope without
/// consuming it.
fn unconsumed(declaration: &str) -> String {
    format!("{RESOURCE}\nfn main()\n    let a {declaration}\n    println(\"end\")\n")
}

/// The scope-exit warning for `a`, its type spelled `spelled`.
fn unconsumed_warning(spelled: &str) -> String {
    format!("resource 'a' of type '{spelled}' was not consumed before scope exit")
}

#[test]
fn an_unconsumed_array_of_a_resource_is_warned_by_its_spelling() {
    assert_compiler_warning(
        &unconsumed("= [Res(1)]"),
        &unconsumed_warning("Array<Res, 1>"),
    );
}

#[test]
fn an_unconsumed_tuple_of_a_resource_is_warned_by_its_spelling() {
    assert_compiler_warning(
        &unconsumed("= (Res(1), 2)"),
        &unconsumed_warning("(Res, int)"),
    );
}

#[test]
fn an_unconsumed_tuple_of_plain_values_is_not_warned() {
    assert_no_diagnostic(&unconsumed("= (1, 2)"), "MER_OWN_001");
}

/// `RESOURCE` and `helper`, then `main` running `body`.
fn with_helper(helper: &str, body: &str) -> String {
    format!("{RESOURCE}\n{helper}\nfn main()\n{body}")
}

#[test]
fn an_array_of_a_resource_returned_from_a_function_runs_its_hook_once() {
    assert_heap_guard_output(
        &with_helper(
            "fn make() [Res; 1]\n    return [Res(1)]\n",
            "    let a = make()\n    println(f\"{a.length()} end\")\n",
        ),
        "1 end\ndrop 1",
    );
}

#[test]
fn a_tuple_of_a_resource_returned_from_a_function_runs_its_hook_once() {
    assert_heap_guard_output(
        &with_helper(
            "fn make() (Res, int)\n    return (Res(1), 2)\n",
            "    let a = make()\n    println(f\"{a.1} end\")\n",
        ),
        "2 end\ndrop 1",
    );
}

/// A class `Holder` keeping a value of type `held` its constructor is given.
fn keeper(held: &str) -> String {
    format!("class Holder\n    public var held {held}\n    fn init(held {held})\n        self.held = held\n")
}

/// Asserts `source` is refused with MER_OWN_003 at the read of `a` after
/// `Holder` kept it, with the exact message.
fn assert_kept_then_read_refused(source: &str) {
    let diagnostic = assert_refused_at(source, "MER_OWN_003", "println(f\"{a.");
    assert_eq!(
        diagnostic.message,
        "'a' was consumed by 'Holder' and cannot be used again\n  fix: use 'a' \
         before the call that keeps it, or pass it to a function that only reads it"
    );
}

#[test]
fn reading_an_array_of_a_resource_a_constructor_kept_is_refused() {
    assert_kept_then_read_refused(&with_helper(
        &keeper("[Res; 1]"),
        "    let a = [Res(1)]\n    let h = Holder(a)\n    println(f\"{a.length()} {h.held.length()}\")\n",
    ));
}

#[test]
fn reading_a_tuple_of_a_resource_a_constructor_kept_is_refused() {
    assert_kept_then_read_refused(&with_helper(
        &keeper("(Res, int)"),
        "    let a = (Res(1), 2)\n    let h = Holder(a)\n    println(f\"{a.1} {h.held.1}\")\n",
    ));
}

#[test]
fn an_array_of_a_resource_lent_to_a_reader_stays_usable_and_runs_its_hook_once() {
    assert_heap_guard_output(
        &with_helper(
            "fn count(a [Res; 1]) int\n    return a.length()\n",
            "    let a = [Res(1)]\n    println(f\"{count(a)} {a.length()} end\")\n",
        ),
        "1 1 end\ndrop 1",
    );
}

#[test]
fn a_tuple_of_a_resource_lent_to_a_reader_stays_usable_and_runs_its_hook_once() {
    assert_heap_guard_output(
        &with_helper(
            "fn second(a (Res, int)) int\n    return a.1\n",
            "    let a = (Res(1), 2)\n    println(f\"{second(a)} {a.1} end\")\n",
        ),
        "2 2 end\ndrop 1",
    );
}

/// A generic class holding both of its arguments in a tuple field.
const DUO: &str = "class Duo<T>\n    public var both (T, T)\n    fn init(first T, second T)\n        self.both = (first, second)\n";

#[test]
fn a_generic_class_holding_a_tuple_of_a_resource_moves() {
    let source = with_helper(
        DUO,
        "    let a = Duo<Res>(Res(1), Res(2))\n    let b = a\n    println(f\"{a.both.0.n}\")\n    println(f\"{b.both.1.n} end\")\n",
    );
    assert_use_after_move_refused(&source);
}

#[test]
fn a_generic_class_holding_a_tuple_of_a_resource_runs_each_hook_once() {
    assert_heap_guard_output(
        &with_helper(
            DUO,
            "    let a = Duo<Res>(Res(1), Res(2))\n    let b = a\n    println(f\"{b.both.1.n} end\")\n",
        ),
        "2 end\ndrop 1\ndrop 2",
    );
}

#[test]
fn a_generic_class_holding_a_tuple_of_plain_values_is_still_copied() {
    assert_heap_guard_output(
        &with_helper(
            DUO,
            "    let a = Duo<int>(1, 2)\n    let b = a\n    println(f\"{a.both.0} {b.both.1}\")\n",
        ),
        "1 2",
    );
}

#[test]
fn cloning_a_list_of_arrays_of_plain_values_copies_them() {
    assert_heap_guard_output(
        "use system.collections.list\n\nfn main()\n    let l = List([[1, 2], [3, 4]])\n    let c = l.clone()\n    println(f\"{c.length()} {c[1][0]} {l[0][1]}\")\n",
        "2 3 2",
    );
}
