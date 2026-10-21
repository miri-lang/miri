// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `sorted_by` on a List sorts a copy in the runtime, calling the comparator
//! closure through a trampoline for every pair it orders. Each element type
//! below reaches the closure out of a different slot representation: a word of
//! its own width, a value held as its bytes, a reference, inline components.

use super::utils::{
    assert_heap_guard_output, assert_runs_with_output, assert_runtime_error,
    get_allocation_count_value,
};

/// The program `allocation_probe` builds: a list of 1024 numbers, sorted by a
/// comparator when `sort` is spelled into it.
fn allocation_probe(sort: &str) -> String {
    format!(
        r#"
use system.collections.list

fn main()
    var values = List<int>()
    values.reserve(1024)
    var i = 0
    while i < 1024
        values.push((i * 7919) % 1024)
        i += 1
    {sort}
    println(f"{{values.length()}}")
"#
    )
}

#[test]
fn sorted_by_allocates_only_the_list_it_returns() {
    let unsorted = get_allocation_count_value(&allocation_probe(""));
    let sorted = get_allocation_count_value(&allocation_probe(
        "values = values.sorted_by(fn(a int, b int) int: a - b)",
    ));
    // Exactly what the wrapper builds: the list it returns, which is a header
    // and the buffer `reserve` gives it, and the comparator closure. A sort
    // that merged through lists of its own would add two blocks per pass.
    assert_eq!(
        sorted - unsorted,
        3,
        "sorting 1024 numbers allocated other blocks than the list and its closure"
    );
}

#[test]
fn sorted_by_keeps_records_with_equal_keys_in_their_order() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Record
    key int
    id int

fn main()
    var l = List<Record>()
    var i = 0
    while i < 40
        l.push(Record(key: (i * 7) % 5, id: i))
        i += 1
    let s = l.sorted_by(fn(a Record, b Record) int: a.key - b.key)
    var ordered = true
    var j = 1
    while j < s.length()
        let p = s[j - 1]
        let q = s[j]
        if p.key > q.key: ordered = false
        if p.key == q.key and p.id > q.id: ordered = false
        j += 1
    println(f"{ordered} {s.length()} {s[0].key} {s[0].id} {s[39].key} {s[39].id}")
    println(f"{l[0].id} {l[39].id}")
"#,
        "true 40 0 0 4 37\n0 39",
    );
}

#[test]
fn sorted_by_orders_records_by_key_then_descending_id() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Record
    key int
    id int

fn next_state(state int) int: (state * 48271) % 2147483647

fn main()
    let n = 3000
    var numbers = List<int>()
    var records = List<Record>()
    var state = 7
    var i = 0
    while i < n
        state = next_state(state)
        numbers.push(state % 1000000)
        records.push(Record(key: state % 1000, id: i))
        i += 1
    numbers.sort()
    let by_key = records.sorted_by(fn(a Record, b Record) int: if a.key != b.key: a.key - b.key else: b.id - a.id)
    var number_hash = 0
    var record_hash = 0
    i = 0
    while i < n
        number_hash = (number_hash * 31 + numbers[i]) % 1000000007
        record_hash = (record_hash * 31 + by_key[i].id) % 1000000007
        i += 1
    println(f"{number_hash} {record_hash}")
"#,
        "450417504 107233592",
    );
}

const STRINGS_BY_LENGTH: &str = r#"
use system.collections.list

fn main()
    var words = List<String>()
    for n in [100, 7, 12345, 42, 9]
        words.push(f"{n}")
    let s = words.sorted_by(fn(a String, b String) int: a.length() - b.length())
    for w in s
        println(w)
    println(f"{words[0]} {words.length()}")
"#;

#[test]
fn sorted_by_orders_strings() {
    assert_runs_with_output(STRINGS_BY_LENGTH, "7\n9\n42\n100\n12345\n100 5");
}

#[test]
fn sorted_by_orders_strings_under_the_heap_guard() {
    assert_heap_guard_output(STRINGS_BY_LENGTH, "7\n9\n42\n100\n12345\n100 5");
}

const CLASSES_BY_WEIGHT: &str = r#"
use system.collections.list

class Item
    var name String
    var weight int

fn main()
    var items = List<Item>()
    items.push(Item(name: f"pear{1}", weight: 3))
    items.push(Item(name: f"fig{2}", weight: 1))
    items.push(Item(name: f"kiwi{3}", weight: 3))
    items.push(Item(name: f"plum{4}", weight: 2))
    let s = items.sorted_by(fn(a Item, b Item) int: a.weight - b.weight)
    for item in s
        println(item.name)
    println(items[0].name)
"#;

#[test]
fn sorted_by_orders_class_instances() {
    assert_runs_with_output(CLASSES_BY_WEIGHT, "fig2\nplum4\npear1\nkiwi3\npear1");
}

#[test]
fn sorted_by_orders_class_instances_under_the_heap_guard() {
    assert_heap_guard_output(CLASSES_BY_WEIGHT, "fig2\nplum4\npear1\nkiwi3\npear1");
}

#[test]
fn sorted_by_calls_a_comparator_capturing_a_string_and_an_int() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let bias = 1
    let tag = f"near{4}"
    let s = List([7, 3, 11, 5, 9]).sorted_by(fn(a int, b int) int: (a + bias) % tag.length() - (b + bias) % tag.length())
    println(f"{s[0]} {s[1]} {s[2]} {s[3]} {s[4]} {tag}")
"#,
        "9 5 11 7 3 near4",
    );
}

#[test]
fn sorted_by_comparator_keeps_what_it_pushes_into_a_captured_list() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Log
    var seen List<String>

    fn init()
        self.seen = List<String>()

    fn record(word String)
        self.seen.push(word)

fn main()
    let log = Log()
    var words = List<String>()
    for n in [100, 7, 12345, 42]
        words.push(f"{n}")
    let by_length = fn(a String, b String) int
        log.record(a)
        return a.length() - b.length()
    let s = words.sorted_by(by_length)
    println(f"{s[0]} {s[1]} {s[2]} {s[3]}")
    var found = true
    for w in log.seen
        if words.index_of(w) == None: found = false
    println(f"{log.seen.length() > 0} {found}")
"#,
        "7 42 100 12345\ntrue true",
    );
}

#[test]
fn sorted_by_takes_a_named_function() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn by_descending(a int, b int) int: b - a

fn main()
    let s = List([2, 9, 4]).sorted_by(by_descending)
    println(f"{s[0]} {s[1]} {s[2]}")
"#,
        "9 4 2",
    );
}

#[test]
fn sorted_by_handles_empty_and_one_element_lists() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let empty = List<int>().sorted_by(fn(a int, b int) int: a - b)
    let one = List([7]).sorted_by(fn(a int, b int) int: a - b)
    println(f"{empty.length()} {one.length()} {one[0]}")
"#,
        "0 1 7",
    );
}

#[test]
fn sorted_by_on_an_array_returns_a_sorted_list() {
    assert_runs_with_output(
        r#"
use system.collections.array
use system.collections.list

fn main()
    let a = [5, 3, 8, 1]
    let s = a.sorted_by(fn(x int, y int) int: x - y)
    println(f"{s.length()} {s[0]} {s[1]} {s[2]} {s[3]} {a[0]}")
"#,
        "4 1 3 5 8 5",
    );
}

#[test]
fn sorted_by_reads_floats_and_narrow_and_wide_integers_at_their_width() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn order_floats(a float, b float) int
    if a < b: return -1
    if a > b: return 1
    return 0

fn main()
    let floats = List([2.5, -1.0, 0.25, -7.5]).sorted_by(order_floats)
    println(f"{floats[0]} {floats[1]} {floats[2]} {floats[3]}")
    var narrow = List<i32>()
    narrow.push(30 as i32)
    narrow.push(-20 as i32)
    narrow.push(10 as i32)
    let ints = narrow.sorted_by(fn(a i32, b i32) int: (a - b) as int)
    println(f"{ints[0]} {ints[1]} {ints[2]}")
    let big i128 = 170141183460469231731687303715884105727
    let low_word_twin i128 = -1
    var wide = List<i128>()
    wide.push(big)
    wide.push(low_word_twin)
    wide.push(0 as i128)
    let w = wide.sorted_by(fn(a i128, b i128) int: if a < b: -1 else: if a > b: 1 else: 0)
    println(f"{w[0] == low_word_twin} {w[1] == 0 as i128} {w[2] == big}")
"#,
        "-7.5 -1.0 0.25 2.5\n-20 10 30\ntrue true true",
    );
}

#[test]
fn sorted_by_orders_structs_with_a_float_field() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Sample
    weight float
    id int

fn main()
    var samples = List<Sample>()
    samples.push(Sample(weight: 0.5, id: 1))
    samples.push(Sample(weight: -2.25, id: 2))
    samples.push(Sample(weight: 1.75, id: 3))
    let s = samples.sorted_by(fn(a Sample, b Sample) int: if a.weight < b.weight: -1 else: if a.weight > b.weight: 1 else: 0)
    println(f"{s[0].id} {s[1].id} {s[2].id} {s[0].weight}")
"#,
        "2 1 3 -2.25",
    );
}

#[test]
fn sorted_by_orders_inline_vectors() {
    assert_heap_guard_output(
        r#"
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec2<f32>>()
    vs.push(Vec2<f32>(3.0, 30.0))
    vs.push(Vec2<f32>(1.0, 10.0))
    vs.push(Vec2<f32>(2.0, 20.0))
    let s = vs.sorted_by(fn(a Vec2<f32>, b Vec2<f32>) int: if a.x < b.x: -1 else: if a.x > b.x: 1 else: 0)
    println(f"{s[0].x} {s[0].y} {s[1].y} {s[2].y}")
"#,
        "1.0 10.0 20.0 30.0",
    );
}

#[test]
fn sorted_by_stops_on_a_comparator_that_overflows() {
    assert_runtime_error(
        r#"
use system.collections.list

fn main()
    let low = -9223372036854775807 - 1
    let s = List([1, low]).sorted_by(fn(a int, b int) int: a - b)
    println(f"{s.length()}")
"#,
        "integer overflow: the result does not fit its type",
    );
}

#[test]
fn sorted_by_with_a_comparator_that_is_not_an_order_returns_a_permutation() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Coin
    var flips int

    fn init()
        self.flips = 0

    fn toss() int
        self.flips = (self.flips * 1103515245 + 12345) % 2147483648
        return self.flips % 3 - 1

fn main()
    let coin = Coin()
    var values = List<int>()
    var i = 0
    while i < 200
        values.push((i * 37) % 200)
        i += 1
    let shuffled = values.sorted_by(fn(a int, b int) int: coin.toss())
    let restored = shuffled.sorted_by(fn(a int, b int) int: a - b)
    var total = 0
    var exact = true
    i = 0
    while i < 200
        total += shuffled[i]
        if restored[i] != i: exact = false
        i += 1
    println(f"{shuffled.length()} {total} {exact}")
"#,
        "200 19900 true",
    );
}

/// `sorted_by` sorts a list of its own, which the comparator cannot reach, so
/// a comparator that grows the receiver changes the receiver only.
#[test]
fn sorted_by_comparator_may_grow_the_receiver() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class H
    var l List<int>

fn grow(h H, a int, b int) int
    h.l.push(a)
    return a - b

fn main()
    let h = H(l: List([5, 4, 3, 2, 1, 9, 8, 7]))
    let s = h.l.sorted_by(fn(a int, b int) int: grow(h, a, b))
    println(f"{s.length()} {s[0]} {s[7]} {h.l.length() > 8}")
"#,
        "8 1 9 true",
    );
}

#[test]
fn sorted_by_called_again_and_again_keeps_every_reference_counted_element() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    var rows = List<List<int>>()
    var i = 0
    while i < 40
        words.push(f"w{(i * 7) % 40}")
        rows.push(List([(i * 13) % 40, i]))
        i += 1
    var first = ""
    var smallest = 0
    var round = 0
    while round < 60
        let by_word = words.sorted_by(fn(a String, b String) int: a.length() - b.length())
        let by_row = rows.sorted_by(fn(a [int], b [int]) int: a[0] - b[0])
        first = by_word[0]
        smallest = by_row[0][1]
        round += 1
    println(f"{first} {smallest} {words.length()} {rows.length()}")
"#,
        "w0 0 40 40",
    );
}

#[test]
fn sorted_by_keeps_equal_keys_in_order_across_merge_passes() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Record
    key int
    id int

fn is_stable(n int) bool
    var l = List<Record>()
    var i = 0
    while i < n
        l.push(Record(key: (i * 7919) % 9, id: i))
        i += 1
    let s = l.sorted_by(fn(a Record, b Record) int: a.key - b.key)
    var j = 1
    while j < n
        let p = s[j - 1]
        let q = s[j]
        if p.key > q.key: return false
        if p.key == q.key and p.id > q.id: return false
        j += 1
    return s.length() == n

fn main()
    for n in [33, 1000, 4097]
        println(f"{n} {is_stable(n)}")
"#,
        "33 true\n1000 true\n4097 true",
    );
}

#[test]
fn sorted_by_sorts_descending_ascending_and_all_equal_input() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn is_ascending(l [int]) bool
    var i = 1
    while i < l.length()
        if l[i - 1] > l[i]: return false
        i += 1
    return true

fn main()
    var descending = List<int>()
    var ascending = List<int>()
    var equal = List<int>()
    var i = 0
    while i < 300
        descending.push(300 - i)
        ascending.push(i)
        equal.push(7)
        i += 1
    let by_value = fn(a int, b int) int: a - b
    let d = descending.sorted_by(by_value)
    let a = ascending.sorted_by(by_value)
    let e = equal.sorted_by(by_value)
    println(f"{is_ascending(d)} {d[0]} {d[299]} {is_ascending(a)} {a[0]} {e.length()} {e[150]}")
"#,
        "true 1 300 true 0 300 7",
    );
}

#[test]
fn sorted_by_orders_by_the_sign_of_the_largest_answers() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn extreme(a int, b int) int
    if a < b: return -9223372036854775807 - 1
    if a > b: return 9223372036854775807
    return 0

fn main()
    let s = List([5, -3, 9, 0, 9, -8]).sorted_by(extreme)
    println(f"{s[0]} {s[1]} {s[2]} {s[3]} {s[4]} {s[5]}")
"#,
        "-8 -3 0 5 9 9",
    );
}
