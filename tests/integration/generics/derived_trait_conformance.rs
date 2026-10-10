// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type the language gives an operator meets the bound of the trait that
//! operator dispatches to, without declaring it: a number is `Comparable`,
//! `Equatable`, `Hashable` and `Addable`; a struct compared field by field is
//! `Equatable` and `Hashable`. The trait's method answers through the
//! operator — `a.compare(b)` orders, `a.equals(b)` is `==`, `a.concat(b)` is
//! `+` — at the type itself and inside a body bounded by the trait. A class
//! meets a bound only by declaring the trait, and a trait of the program's
//! own is never met by a method it happens to share a name with.

use super::utils::*;

fn identity_through(bound: &str) -> String {
    format!(
        "fn f<T implements {bound}>(x T) T
    return x

fn main()
    let a f32 = 1.5
    println(f\"{{f(a)}}\")
    println(f\"{{f(7)}}\")
"
    )
}

#[test]
fn a_number_meets_each_operator_trait_bound() {
    for bound in ["Comparable", "Equatable", "Hashable", "Addable"] {
        assert_runs_with_output(&identity_through(bound), "1.5\n7");
    }
}

#[test]
fn a_number_meets_the_accelerable_bound() {
    let code = format!(
        "use system.accelerator\n{}",
        identity_through("Accelerable")
    );
    assert_runs_with_output(&code, "1.5\n7");
}

#[test]
fn a_bounded_body_orders_numbers_through_compare() {
    let code = "
fn order<T implements Comparable>(a T, b T) int
    return a.compare(b)

fn main()
    let x f32 = 2.5
    let y f32 = 1.5
    println(f\"{order(3, 5)} {order(5, 5)} {order(9, 2)} {order(x, y)} {order(false, true)}\")
";
    assert_runs_with_output(code, "-1 0 1 1 -1");
}

#[test]
fn a_bounded_body_reads_equals_concat_and_hash_on_numbers() {
    let code = "
fn same<T implements Equatable>(a T, b T) bool
    return a.equals(b)

fn sum<T implements Addable>(a T, b T) T
    return a.concat(b)

fn stable<T implements Hashable>(a T) bool
    return a.hash() == a.hash()

fn main()
    let half f64 = 0.5
    println(f\"{same(4, 4)} {same(4, 5)} {sum(3, 4)} {sum(half, half)} {stable(42)}\")
";
    assert_runs_with_output(code, "true false 7 1.0 true");
}

#[test]
fn a_bounded_body_uses_the_operator_at_a_number() {
    let code = "
fn largest<T implements Comparable>(a T, b T) T
    if a < b:
        return b
    return a

fn main()
    println(f\"{largest(3, 8)} {largest(2.5, 1.5)}\")
";
    assert_runs_with_output(code, "8 2.5");
}

#[test]
fn a_number_answers_the_operator_methods_directly() {
    let code = "
fn main()
    let a = 3
    let b f32 = 1.5
    println(f\"{a.compare(4)} {a.compare(3)} {b.compare(1.0)} {a.equals(3)} {a.concat(4)} {a.repeat(5)}\")
";
    assert_runs_with_output(code, "-1 0 1 true 7 15");
}

#[test]
fn a_receiver_with_an_effect_is_evaluated_once_by_compare() {
    let code = "
class Counter
    public var calls int
    public fn next() int
        self.calls = self.calls + 1
        return self.calls

fn main()
    var counter = Counter(calls: 0)
    let order = counter.next().compare(counter.next())
    println(f\"{order} {counter.calls}\")
";
    assert_runs_with_output(code, "-1 2");
}

#[test]
fn a_struct_compared_field_by_field_meets_equatable_and_hashable() {
    let code = "
struct Point
    x int
    y int

fn same<T implements Equatable>(a T, b T) bool
    return a.equals(b)

fn stable<T implements Hashable>(a T) bool
    return a.hash() == a.hash()

fn main()
    println(f\"{same(Point(1, 2), Point(1, 2))} {same(Point(1, 2), Point(2, 1))} {stable(Point(3, 4))}\")
";
    assert_heap_guard_output(code, "true false true");
}

#[test]
fn a_bounded_body_calls_the_trait_method_a_class_declares() {
    let code = "
class Weight implements Comparable, Addable
    public var grams int
    public fn compare(other Self) int
        return self.grams - other.grams
    public fn concat(other Self) Self
        return Weight(grams: self.grams + other.grams)

fn order<T implements Comparable>(a T, b T) int
    return a.compare(b)

fn sum<T implements Addable>(a T, b T) T
    return a.concat(b)

fn main()
    let total = sum(Weight(grams: 2), Weight(grams: 5))
    println(f\"{order(Weight(grams: 1), Weight(grams: 5))} {total.grams}\")
";
    assert_heap_guard_output(code, "-4 7");
}

#[test]
fn a_bool_does_not_meet_addable() {
    let code = "
fn f<T implements Addable>(x T) T
    return x

fn main()
    println(f\"{f(true)}\")
";
    assert_compiler_error(
        code,
        "Type bool does not satisfy constraint implements Addable",
    );
}

#[test]
fn a_float_does_not_meet_multiplicable() {
    let code = "
fn f<T implements Multiplicable>(x T) T
    return x

fn main()
    let a f32 = 1.5
    println(f\"{f(a)}\")
";
    assert_compiler_error(
        code,
        "Type f32 does not satisfy constraint implements Multiplicable",
    );
}

#[test]
fn a_struct_does_not_meet_comparable() {
    let code = "
struct Point
    x int

fn f<T implements Comparable>(x T) T
    return x

fn main()
    println(f\"{f(Point(1)).x}\")
";
    assert_compiler_error(
        code,
        "Type Point does not satisfy constraint implements Comparable",
    );
}

#[test]
fn a_class_that_does_not_declare_equatable_does_not_meet_it() {
    let code = "
class Box
    public var v int

fn f<T implements Equatable>(x T) T
    return x

fn main()
    println(f\"{f(Box(v: 1)).v}\")
";
    assert_compiler_error(
        code,
        "Type Box does not satisfy constraint implements Equatable",
    );
}

#[test]
fn a_programs_own_trait_is_not_met_by_a_shared_method_name() {
    let code = "
trait Rankable
    fn compare(other int) int

fn f<T implements Rankable>(x T) T
    return x

fn main()
    println(f\"{f(3)}\")
";
    assert_compiler_error(
        code,
        "Type int does not satisfy constraint implements Rankable",
    );
}

#[test]
fn a_bool_has_no_concat() {
    let code = "
fn main()
    println(f\"{true.concat(false)}\")
";
    assert_compiler_error(code, "Type 'bool' does not have members");
}

#[test]
fn compare_refuses_an_argument_of_another_type() {
    let code = "
fn main()
    println(f\"{3.compare(true)}\")
";
    assert_compiler_error(code, "Type mismatch");
}

#[test]
fn compare_orders_unsigned_values_by_value_and_leaves_nan_unordered() {
    let code = "
fn main()
    let big u8 = 200
    let small u8 = 1
    let zero f64 = 0.0
    let nan = zero / zero
    println(f\"{big.compare(small)} {small.compare(big)} {nan.compare(1.0)}\")
";
    assert_runs_with_output(code, "1 -1 0");
}

#[test]
fn a_string_meets_the_bounds_its_class_declares() {
    let code = "
fn order<T implements Comparable>(a T, b T) int
    return a.compare(b)

fn same<T implements Equatable>(a T, b T) bool
    return a.equals(b)

fn main()
    let ordered = order(\"b\", \"a\")
    let matched = same(\"x\", \"x\")
    println(f\"{ordered} {matched}\")
";
    assert_heap_guard_output(code, "1 true");
}

#[test]
fn equals_on_a_struct_holding_a_list_releases_both_values() {
    let code = "
use system.collections.list

struct Bag
    items List<int>
    name String

fn same<T implements Equatable>(a T, b T) bool
    return a.equals(b)

fn main()
    let equal = same(Bag(List([1, 2]), \"a\"), Bag(List([1, 2]), \"a\"))
    let differ = Bag(List([1]), \"a\").equals(Bag(List([2]), \"a\"))
    println(f\"{equal} {differ}\")
";
    assert_heap_guard_output(code, "true false");
}
