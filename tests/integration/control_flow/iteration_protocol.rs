// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `for` over a value that is `Iterable` but not `Indexed`: the loop asks the
//! value for its `iterator()` once and calls `next()` before each pass, until
//! `next()` returns `None`. A second loop variable reads the iterator's
//! `Entries.value()`. Set and map order is unspecified, so these tests assert
//! sums, counts and sorted output.

use super::utils::*;

/// A user class iterated only through its `iterator()`: it counts down from
/// `start` to 1, handing out each number as a string built at run time.
const COUNTDOWN: &str = r##"
use system.ops

class Countdown implements Iterable<String>
    var start int
    fn init(start int)
        self.start = start
    public fn iterator() Iterator<String>
        return CountdownIterator(self.start)

class CountdownIterator implements Iterator<String>
    var left int
    fn init(left int)
        self.left = left
    public fn next() String?
        if self.left <= 0: return None
        self.left -= 1
        return Some(f"n{self.left + 1}")
"##;

fn with_countdown(main: &str) -> String {
    format!("{COUNTDOWN}\n{main}")
}

#[test]
fn a_loop_over_a_user_iterable_walks_its_iterator() {
    assert_heap_guard_output(
        &with_countdown(
            r##"
fn main()
    for word in Countdown(3)
        println(word)
"##,
        ),
        "n3\nn2\nn1",
    );
}

#[test]
fn a_loop_over_an_empty_user_iterable_runs_no_pass() {
    assert_heap_guard_output(
        &with_countdown(
            r##"
fn main()
    var passes = 0
    for word in Countdown(0)
        passes += 1
    println(f"{passes}")
"##,
        ),
        "0",
    );
}

#[test]
fn break_continue_and_return_inside_a_protocol_loop_release_what_it_holds() {
    assert_heap_guard_output(
        &with_countdown(
            r##"
fn first_long(c Countdown) String
    for word in c
        if word == "n4": continue
        if word.length() == 2: return word
    return "none"

fn main()
    for word in Countdown(5)
        if word == "n3": break
        println(word)
    println(first_long(Countdown(4)))
"##,
        ),
        "n5\nn4\nn3",
    );
}

#[test]
fn a_loop_over_a_set_visits_every_element_once() {
    assert_heap_guard_output(
        r##"
use system.collections.set

fn main()
    var sum = 0
    var passes = 0
    for x in {10, 20, 30, 40}
        sum += x
        passes += 1
    println(f"{sum} {passes}")
"##,
        "100 4",
    );
}

#[test]
fn a_loop_over_a_set_of_strings_releases_each_element() {
    assert_heap_guard_output(
        r##"
use system.collections.set
use system.collections.list

fn main()
    var s = Set<String>()
    s.add("pe" + "ar")
    s.add("fig")
    s.add("ap" + "ple")
    var seen = List<String>()
    for word in s
        if word == "fig": continue
        seen.push(word)
    seen.sort()
    for word in seen
        println(word)
"##,
        "apple\npear",
    );
}

#[test]
fn break_and_return_inside_a_set_loop_release_the_iterator() {
    assert_heap_guard_output(
        r##"
use system.collections.set

fn has_long(s {String}) bool
    for word in s
        if word.length() > 3: return true
    return false

fn main()
    let s = {"a" + "b", "c" + "dddd", "e"}
    var passes = 0
    for word in s
        passes += 1
        break
    println(f"{passes} {has_long(s)}")
"##,
        "1 true",
    );
}

#[test]
fn a_loop_over_an_empty_set_runs_no_pass() {
    assert_heap_guard_output(
        r##"
use system.collections.set

fn main()
    var passes = 0
    for word in Set<String>()
        passes += 1
    println(f"{passes}")
"##,
        "0",
    );
}

#[test]
fn writing_to_a_set_inside_its_own_loop_walks_the_original() {
    assert_heap_guard_output(
        r##"
use system.collections.set

fn main()
    var s = {1, 2, 3}
    var passes = 0
    for x in s
        s.add(x + 100)
        passes += 1
    println(f"{passes} {s.length()}")
"##,
        "3 6",
    );
}

#[test]
fn a_loop_over_a_map_yields_its_keys() {
    assert_heap_guard_output(
        r##"
use system.collections.map
use system.collections.list

fn main()
    let m = {"b": 2, "a": 1, "c": 3}
    var keys = List<String>()
    for k in m
        keys.push(k)
    keys.sort()
    for k in keys
        println(k)
"##,
        "a\nb\nc",
    );
}

#[test]
fn a_two_variable_loop_over_a_map_yields_keys_and_values() {
    assert_heap_guard_output(
        r##"
use system.collections.map
use system.collections.list

fn main()
    let m = {"b": "two", "a": "one", "c": "three"}
    var lines = List<String>()
    for k, v in m
        if k == "c": continue
        lines.push(f"{k}={v}")
    lines.sort()
    for line in lines
        println(line)
"##,
        "a=one\nb=two",
    );
}

#[test]
fn break_inside_a_two_variable_map_loop_releases_key_and_value() {
    assert_heap_guard_output(
        r##"
use system.collections.map

fn total(m {String: int}) int
    var sum = 0
    for k, v in m
        if v < 0: return -1
        sum += v
    return sum

fn main()
    let m = {"x": 1, "y": 2}
    var passes = 0
    for k, v in m
        passes += 1
        break
    println(f"{passes} {total(m)}")
"##,
        "1 3",
    );
}

#[test]
fn a_two_variable_loop_over_an_empty_map_runs_no_pass() {
    assert_heap_guard_output(
        r##"
use system.collections.map

fn main()
    var passes = 0
    for k, v in Map<String, String>()
        passes += 1
    println(f"{passes}")
"##,
        "0",
    );
}

#[test]
fn a_protocol_loop_in_a_generic_body_walks_each_instantiation() {
    assert_heap_guard_output(
        r##"
use system.collections.set
use system.collections.map

fn count<T>(s {T}) int
    var n = 0
    for x in s
        n += 1
    return n

fn sum_values<K>(m {K: int}) int
    var sum = 0
    for k, v in m
        sum += v
    return sum

fn main()
    let words = {"a" + "b", "c"}
    let named = {"a": 1, "b": 2}
    println(f"{count({1, 2, 3})} {count(words)}")
    println(f"{sum_values(named)} {sum_values({7: 10, 8: 20})}")
"##,
        "3 2\n3 30",
    );
}

#[test]
fn a_generic_user_iterable_is_walked_at_each_instantiation() {
    assert_heap_guard_output(
        r##"
use system.ops
use system.collections.list

class Bag<T> implements Iterable<T>
    var items [T]
    fn init(items [T])
        self.items = items
    public fn iterator() Iterator<T>
        return BagIterator<T>(self.items)

class BagIterator<T> implements Iterator<T>
    var items [T]
    var at int
    fn init(items [T])
        self.items = items
        self.at = 0
    public fn next() T?
        if self.at >= self.items.length(): return None
        self.at += 1
        return Some(self.items[self.at - 1])

fn main()
    for word in Bag<String>(List(["x" + "y", "z"]))
        println(word)
    var sum = 0
    for n in Bag<int>(List([4, 5]))
        sum += n
    println(f"{sum}")
"##,
        "xy\nz\n9",
    );
}

#[test]
fn a_two_variable_loop_reads_the_value_of_a_user_iterator_with_entries() {
    assert_heap_guard_output(
        r##"
use system.ops

class Squares implements Iterable<String>
    var count int
    fn init(count int)
        self.count = count
    public fn iterator() Iterator<String>
        return SquaresIterator(self.count)

class SquaresIterator implements Iterator<String>, Entries<int>
    var left int
    var at int
    fn init(count int)
        self.left = count
        self.at = 0
    public fn next() String?
        if self.at >= self.left: return None
        self.at += 1
        return Some(f"#{self.at}")
    public fn value() int?
        if self.at == 0: return None
        return Some(self.at * self.at)

fn main()
    for name, square in Squares(3)
        println(f"{name} {square}")
"##,
        "#1 1\n#2 4\n#3 9",
    );
}

#[test]
fn a_class_implementing_iterable_with_only_length_and_element_at_is_told_to_implement_indexed() {
    assert_compiler_error(
        r##"
use system.ops

class Words implements Iterable<String>
    var count int
    fn init(count int)
        self.count = count
    public fn length() int
        return self.count
    public fn element_at(index int) String
        return "w"

fn main()
    for w in Words(2)
        println(w)
"##,
        "implement `Indexed<String>`",
    );
}

#[test]
fn a_two_variable_loop_over_a_set_is_refused() {
    assert_compiler_error(
        r##"
use system.collections.set

fn main()
    for x, i in {1, 2, 3}
        println(f"{x} {i}")
"##,
        "has no value to give a second loop variable",
    );
}

#[test]
fn a_two_variable_loop_over_a_user_iterator_without_entries_is_refused() {
    assert_compiler_error(
        &with_countdown(
            r##"
fn main()
    for word, extra in Countdown(2)
        println(word)
"##,
        ),
        "has no value to give a second loop variable",
    );
}

#[test]
fn a_protocol_loop_in_device_code_is_refused() {
    assert_compiler_error(
        r##"
use system.collections.set
use system.gpu

gpu fn k(n int)
    var acc = n
    for v in {1, 2}
        acc += v

fn main()
    println("x")
"##,
        "walks its iterator, which device code cannot run",
    );
}

/// A user iterable whose iterator implements `Entries<String>` and whose
/// `value()` is `None` once the `stop`th element is reached, which ends the
/// loop before that pass runs.
const PAIRS: &str = r##"
use system.ops

class Pairs implements Iterable<String>
    var n int
    var stop int
    fn init(n int, stop int)
        self.n = n
        self.stop = stop
    public fn iterator() Iterator<String>
        return PairsIterator(self.n, self.stop)

class PairsIterator implements Iterator<String>, Entries<String>
    var i int
    var n int
    var stop int
    fn init(n int, stop int)
        self.n = n
        self.stop = stop
        self.i = 0
    public fn next() String?
        if self.i >= self.n: return None
        self.i += 1
        return Some(f"k{self.i}")
    public fn value() String?
        if self.i == self.stop: return None
        return Some(f"v{self.i}")
"##;

#[test]
fn a_value_of_none_after_the_first_pass_ends_the_loop_releasing_each_value_once() {
    assert_heap_guard_output(
        &format!(
            "{PAIRS}\n{}",
            r##"
fn main()
    for k, v in Pairs(5, 3)
        println(f"{k}={v}")
    println("done")
"##
        ),
        "k1=v1\nk2=v2\ndone",
    );
}

#[test]
fn a_value_of_none_on_the_first_pass_ends_the_loop_before_any_pass() {
    assert_heap_guard_output(
        &format!(
            "{PAIRS}\n{}",
            r##"
fn main()
    for k, v in Pairs(5, 1)
        println(f"{k}={v}")
    println("done")
"##
        ),
        "done",
    );
}

#[test]
fn a_two_variable_loop_reads_an_iterator_declared_after_the_loop() {
    assert_heap_guard_output(
        r##"
use system.ops

fn main()
    for name, square in Squares(2)
        println(f"{name} {square}")

class Squares implements Iterable<String>
    var count int
    fn init(count int)
        self.count = count
    public fn iterator() Iterator<String>
        return SquaresIterator(self.count)

class SquaresIterator implements Iterator<String>, Entries<int>
    var at int
    var count int
    fn init(count int)
        self.count = count
        self.at = 0
    public fn next() String?
        if self.at >= self.count: return None
        self.at += 1
        return Some(f"#{self.at}")
    public fn value() int?
        return Some(self.at * self.at)
"##,
        "#1 1\n#2 4",
    );
}

/// A base iterable whose subclass overrides `iterator()` to hand out a
/// different iterator class.
const OVERRIDDEN: &str = r##"
use system.ops

class Base implements Iterable<int>
    fn init()
        let unused = 0
    public fn iterator() Iterator<int>
        return One(1)

class Sub extends Base
    fn init()
        super.init()
    public fn iterator() Iterator<int>
        return Two(2)

class One implements Iterator<int>, Entries<int>
    var v int
    var done bool
    fn init(v int)
        self.v = v
        self.done = false
    public fn next() int?
        if self.done: return None
        self.done = true
        return Some(self.v)
    public fn value() int?
        return Some(self.v * 10)

class Two implements Iterator<int>, Entries<int>
    var v int
    var left int
    fn init(v int)
        self.v = v
        self.left = 2
    public fn next() int?
        if self.left == 0: return None
        self.left -= 1
        return Some(self.v)
    public fn value() int?
        return Some(self.v * 100)
"##;

#[test]
fn a_loop_over_a_base_class_value_walks_the_iterator_its_subclass_overrides() {
    assert_heap_guard_output(
        &format!(
            "{OVERRIDDEN}\n{}",
            r##"
fn walk(b Base)
    for x in b
        println(f"{x}")

fn main()
    walk(Sub())
    walk(Base())
"##
        ),
        "2\n2\n1",
    );
}

#[test]
fn a_two_variable_loop_over_a_class_whose_subclass_overrides_iterator_is_refused() {
    assert_compiler_error(
        &format!(
            "{OVERRIDDEN}\n{}",
            r##"
fn walk(b Base)
    for x, v in b
        println(f"{x} {v}")

fn main()
    walk(Sub())
"##
        ),
        "has no value to give a second loop variable",
    );
}
