// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// The default leak check every test runs under (`MIRI_LEAK_CHECK=1`) counts
// every block a program allocates: the blocks the runtime allocates with a
// reference-count header, and the blocks compiled code allocates itself — a
// class instance, a tuple, an enum payload, a closure environment. A block of
// either kind left at exit fails the run.
//
// `MIRI_HOOK_CHECK=1` adds the check that a value whose release runs a drop
// hook never gains a second holder.

use super::super::utils::*;

/// A class block nothing releases fails the default run.
#[test]
fn a_leaked_class_block_fails_the_default_leak_check() {
    assert_leak_detected(
        r#"
use system.testing

fn main()
    simulate_closure_leak()
"#,
        "leaked 1 inline block(s)",
    );
}

/// The report counts every leaked block.
#[test]
fn the_leak_check_counts_each_leaked_class_block() {
    assert_leak_detected(
        r#"
use system.testing

fn main()
    simulate_closure_leak()
    simulate_closure_leak()
"#,
        "leaked 2 inline block(s)",
    );
}

/// Two class instances that hold each other are never released, and the
/// default leak check sees both.
#[test]
fn two_instances_holding_each_other_fail_the_default_leak_check() {
    assert_leak_detected(
        r#"
use system.io

class Node
    public var value int
    public var next Node?
    public fn init(value int)
        self.value = value
        self.next = None

fn main()
    var a = Node(1)
    var b = Node(2)
    a.next = Some(b)
    b.next = Some(a)
    println("linked")
"#,
        "leaked 2 inline block(s)",
    );
}

/// Instances, tuples and closure environments released on every path leave
/// both balances even.
#[test]
fn released_class_tuple_and_closure_blocks_pass_the_leak_check() {
    assert_runs_with_output(
        r#"
use system.io

class Box
    public var value int
    public fn init(value int)
        self.value = value

fn main()
    let b = Box(3)
    let pair = (b.value, "four")
    let read = fn() int: b.value + pair.0
    println(f"{read()}")
"#,
        "6",
    );
}

/// A block whose release runs a drop hook has one holder; the test helper that
/// gives it a second stops the run, whether or not the check was asked for.
#[test]
fn a_hooked_block_given_a_second_holder_stops_the_run() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    simulate_shared_hooked_block()
"#,
        "gained a second holder",
    );
}

/// Under the hook check, a resource moved from binding to binding, returned
/// from a function and wrapped in an optional local keeps one holder, and its
/// hook runs once.
#[test]
fn a_moved_resource_keeps_one_holder_under_the_hook_check() {
    assert_hook_check_output(
        r#"
use system.io

class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

fn make(id int) Token
    let t = Token(id: id)
    return t

fn main()
    let t = make(1)
    let moved = t
    let held = Some(moved)
    println("held")
"#,
        "held\nreleased 1",
    );
}
