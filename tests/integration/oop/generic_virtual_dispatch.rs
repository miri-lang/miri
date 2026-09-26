// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A call through a trait receiver to an instance of a generic class reaches
//! the body compiled for that instance's own type arguments — the same body a
//! direct call on the class names — so a managed argument is counted, a
//! scalar keeps its width and a struct keeps its fields.

use super::utils::*;

#[test]
fn test_a_generic_class_method_reached_through_a_string_receiver_counts_its_values() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn keep(a T, b T) T

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

fn main()
    let o Op<String> = Impl<String>()
    println(o.keep("a" + "b", "c" + "d"))
"#,
        "cd",
    );
}

#[test]
fn test_a_generic_class_method_reached_through_a_struct_receiver_keeps_its_fields() {
    assert_heap_guard_output(
        r#"
use system.io

struct Pt
    x int
    y int

trait Op<T>
    fn keep(a T, b T) T

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

fn main()
    let o Op<Pt> = Impl<Pt>()
    let p = o.keep(Pt(x: 1, y: 2), Pt(x: 3, y: 4))
    println(f"{p.x} {p.y}")
"#,
        "3 4",
    );
}

#[test]
fn test_a_generic_class_method_reached_through_scalar_receivers_keeps_their_width() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn keep(a T, b T) T

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

fn main()
    let i Op<int> = Impl<int>()
    println(f"{i.keep(1, 2)}")
    let a f32 = 1.5
    let b f32 = 2.25
    let f Op<f32> = Impl<f32>()
    println(f"{f.keep(a, b)}")
    let w i128 = -18446744073709551621
    let v i128 = 7
    let n Op<i128> = Impl<i128>()
    let back = n.keep(v, w)
    println(f"{back == w} {back == -5}")
"#,
        "2\n2.25\ntrue false",
    );
}

/// A trait default reached through a receiver of a generic implementor runs
/// the copy compiled at that implementor's arguments: overwriting a `String`
/// local releases the replaced value exactly once.
#[test]
fn test_a_trait_default_reached_through_a_string_receiver_of_a_generic_class_counts_its_values() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl<T> implements Op<T>

fn show(o Op<String>)
    println(o.keep("p" + "q", "r" + "s"))

fn main()
    show(Impl<String>())
"#,
        "rs",
    );
}

#[test]
fn test_a_trait_default_reached_through_struct_and_scalar_receivers_of_a_generic_class() {
    assert_heap_guard_output(
        r#"
use system.io

struct Pt
    x int
    y int

trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl<T> implements Op<T>

fn main()
    let o Op<Pt> = Impl<Pt>()
    let p = o.keep(Pt(x: 1, y: 2), Pt(x: 3, y: 4))
    println(f"{p.x} {p.y}")
    let i Op<int> = Impl<int>()
    println(f"{i.keep(1, 2)}")
    let a f32 = 1.5
    let b f32 = 2.25
    let f Op<f32> = Impl<f32>()
    println(f"{f.keep(a, b)}")
    let w i128 = -18446744073709551621
    let v i128 = 7
    let n Op<i128> = Impl<i128>()
    let back = n.keep(v, w)
    println(f"{back == w} {back == -5}")
"#,
        "3 4\n2\n2.25\ntrue false",
    );
}

/// A concrete generic base: the subclass pins its parameter, and both the
/// base's own method and the trait default it supplies are reached through a
/// receiver at the pinned type.
#[test]
fn test_a_concrete_generic_base_pinned_by_a_subclass_answers_through_a_string_receiver() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn pick(a T, b T) T
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Base<U> implements Op<U>
    fn pick(a U, b U) U
        var x U = b
        x = a
        return x

class Sub extends Base<String>

fn show(o Op<String>)
    println(o.keep("p" + "q", "r" + "s"))
    println(o.pick("p" + "q", "r" + "s"))

fn main()
    show(Sub())
    show(Base<String>())
"#,
        "rs\npq\nrs\npq",
    );
}

/// Two instantiations of one class alive together each answer with the body
/// compiled for their own argument.
#[test]
fn test_two_instantiations_of_one_class_answer_through_their_own_receivers() {
    assert_heap_guard_output(
        r#"
use system.io

trait Get<T>
    fn get() T
    fn again() T
        let x T = self.get()
        return x

class Cell<T> implements Get<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn words(g Get<String>) String
    return f"{g.get()} {g.again()}"

fn number(g Get<int>) int
    return g.get() + g.again()

fn fraction(g Get<float>) float
    return g.get() + g.again()

fn main()
    let s = Cell<String>("a" + "b")
    let n = Cell<int>(7)
    let f = Cell<float>(0.25)
    println(words(s))
    println(f"{number(n)} {fraction(f)}")
"#,
        "ab ab\n14 0.5",
    );
}

/// Receivers kept in a list and called in a loop reach each element's body.
#[test]
#[ignore = "a collection of trait-typed elements does not link: its element \
            release names a `__decref_{Trait}` helper that is never generated, \
            whatever the implementing classes are"]
fn test_trait_receivers_in_a_list_reach_their_generic_class_bodies() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl<T> implements Op<T>

class Loud<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = b
        x = a
        return x

fn main()
    var ops = List<Op<String>>()
    ops.push(Impl<String>())
    ops.push(Loud<String>())
    for o in ops
        println(o.keep("p" + "q", "r" + "s"))
"#,
        "rs\npq",
    );
}

/// Releasing a trait receiver that holds a generic class instance releases the
/// managed field the instance keeps at its own argument.
#[test]
#[ignore = "releasing a class instance through a trait-typed binding frees the \
            object but none of its managed fields, for a generic and a plain \
            class alike: the release has no per-instance drop to dispatch to"]
fn test_releasing_a_trait_receiver_releases_the_generic_instance_fields() {
    assert_heap_guard_output(
        r#"
use system.io

trait Get<T>
    fn get() T

class Cell<T> implements Get<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let _held Get<String> = Cell<String>("a" + "b")
    println("built")
"#,
        "built",
    );
}

/// How long the compiler may take to refuse a program that reaches a class at
/// ever-growing type arguments before it is taken to be looping.
const POLYMORPHIC_RECURSION_BUDGET: std::time::Duration = std::time::Duration::from_secs(90);

/// Assert that building `code` is refused with the diagnostic `expected_code`
/// within [`POLYMORPHIC_RECURSION_BUDGET`], killing the compiler when it
/// overruns so a program the compiler cannot finish specializing fails this
/// test instead of hanging the suite. The report must carry each of
/// `fragments`, and spells a type the way the source does, never as the
/// compiler's `List(String)`.
fn assert_refused_within_budget(code: &str, expected_code: &str, fragments: &[&str]) {
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "{code}").unwrap();
    let stdlib_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");
    let started = std::time::Instant::now();
    let output = crate::utils::miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib_path)
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .arg("build")
        .arg(file.path())
        .timeout(POLYMORPHIC_RECURSION_BUDGET)
        .output()
        .unwrap();
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !output.status.success() && report.contains(expected_code),
        "expected {expected_code} within {POLYMORPHIC_RECURSION_BUDGET:?}, got {:?} after {:?}:\n{report}",
        output.status,
        started.elapsed(),
    );
    for fragment in fragments {
        assert!(
            report.contains(fragment),
            "expected the report to carry {fragment:?}:\n{report}"
        );
    }
    for internal in ["List(", "Wrap(", "Box("] {
        assert!(
            !report.contains(internal),
            "the report spells a type as {internal:?}:\n{report}"
        );
    }
}

/// A method that builds its own class at a wrapped argument and calls it
/// through a trait reaches a new instantiation on every call: specializing
/// one body per level would never end, and running the class's shared body
/// at a concrete argument would treat a managed value as a plain word. The
/// program is refused.
#[test]
fn test_a_class_reaching_itself_at_a_growing_argument_through_a_trait_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

trait Op<T>
    fn depth(n int) int

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Impl<T> implements Op<T>
    fn depth(n int) int
        if n == 0
            return 0
        let inner Op<Wrap<T>> = Impl<Wrap<T>>()
        return 1 + inner.depth(n - 1)

fn main()
    let o Op<int> = Impl<int>()
    println(f"{o.depth(3)}")
"#,
        "MER_MIR_016",
        &[
            "instantiating `Impl<Wrap<Wrap<…>>>` nests its type argument 33 levels deep",
            "each call to `Impl.depth` builds `Impl` at a larger type: \
             Impl<int> → Impl<Wrap<int>> → Impl<Wrap<Wrap<int>>> → …",
            "the type argument grows on every call through the trait",
        ],
    );
}

/// Two growing arguments per level double the instantiations each level
/// reaches; the refusal comes before the doubling runs away.
#[test]
fn test_a_class_reaching_itself_at_two_growing_arguments_through_a_trait_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

trait Op<T>
    fn depth(n int) int

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Box<T>
    v T
    fn init(v T)
        self.v = v

class Impl<T> implements Op<T>
    fn depth(n int) int
        if n == 0
            return 1
        let a Op<Wrap<T>> = Impl<Wrap<T>>()
        let b Op<Box<T>> = Impl<Box<T>>()
        return a.depth(n - 1) + b.depth(n - 1)

fn main()
    let o Op<int> = Impl<int>()
    println(f"{o.depth(3)}")
"#,
        "MER_MIR_016",
        &[
            "nests its type argument 33 levels deep",
            "Impl<int> → Impl<",
        ],
    );
}

/// A call through a receiver that pins no argument reaches every instance the
/// program builds, so a class that builds itself at a wrapped managed argument
/// inside the method that call reads grows without end, and is refused rather
/// than run through the shared body that would release the `String` twice.
#[test]
fn test_a_class_growing_a_managed_argument_behind_an_unpinned_receiver_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

trait Op
    fn depth(n int) int

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Impl<T> implements Op
    v T
    fn init(v T)
        self.v = v
    fn depth(n int) int
        if n == 0
            return 0
        let a = Impl<Wrap<T>>(Wrap<T>(self.v))
        return 1 + call(a, n - 1)

fn call(o Op, n int) int
    return o.depth(n)

fn main()
    let o = Impl<String>("hi" + "x")
    println(f"{call(o, 2)}")
"#,
        "MER_MIR_016",
        &[
            "instantiating `Impl<Wrap<Wrap<…>>>` nests its type argument 33 levels deep",
            "Impl<String> → Impl<Wrap<String>> → Impl<Wrap<Wrap<String>>> → …",
        ],
    );
}

/// An instance built one level deeper inside the body a trait call reached
/// runs the bodies compiled for its own argument: `first` on the
/// `Vec<List<String>>` hands out a counted reference to the list it holds,
/// where the class's shared body would hand out an uncounted one and the
/// list would be released twice.
#[test]
fn test_an_instance_grown_one_level_inside_a_dispatched_body_runs_its_own_bodies() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

trait Seq<T>
    fn first() T
    fn nested() Vec<List<T>>

class Vec<T> implements Seq<T>
    items List<T>
    fn init(items List<T>)
        self.items = items
    fn first() T
        return self.items.element_at(0)
    fn nested() Vec<List<T>>
        let outer = List<List<T>>()
        outer.push(self.items)
        return Vec<List<T>>(outer)

fn grow(s Seq<String>) Vec<List<String>>
    return s.nested()

fn peek(q Seq<List<String>>) int
    let inner = q.first()
    return inner.length()

fn main()
    let s = Vec<String>(List(["a" + "b", "c" + "d", "e" + "f"]))
    let n = grow(s)
    println(f"{peek(n)} {peek(n)}")
"#,
        "3 3",
    );
}

/// A method that builds its class at a nested argument, called through
/// receivers pinned to each level the program asks for, grows only as far as
/// those calls go and is not refused. Run untracked: the instances are held
/// through trait-typed bindings, whose release leaves the fields behind (see
/// the ignored release test above).
#[test]
fn test_a_class_nesting_its_argument_as_far_as_its_callers_reach_is_not_refused() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list

trait Seq<T>
    fn size() int
    fn nested() Seq<List<T>>

class Vec<T> implements Seq<T>
    items List<T>
    fn init(items List<T>)
        self.items = items
    fn size() int
        return self.items.length()
    fn nested() Seq<List<T>>
        let outer = List<List<T>>()
        outer.push(self.items)
        outer.push(self.items)
        return Vec<List<T>>(outer)

fn main()
    let s Seq<String> = Vec<String>(List(["a" + "b", "c" + "d", "e" + "f"]))
    let n = s.nested()
    println(f"{n.size()}")
    let m = n.nested()
    println(f"{m.size()}")
"#,
        "2\n2",
    );
}

/// A trait default no call reaches at an instantiation is never compiled
/// there: `Bag<Pt>` inherits a `sum` over elements that do not add, and only
/// the `int` bag is summed through the trait.
#[test]
fn test_a_trait_default_dispatched_at_one_instantiation_is_not_compiled_at_another() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.foldable

struct Pt
    x int

class Bag<T> implements Foldable<T>
    v T
    fn init(v T)
        self.v = v
    fn length() int
        return 1
    fn element_at(index int) T
        return self.v

fn total(f Foldable<int>) int
    return f.sum() ?? 0

fn main()
    let p = Bag<Pt>(Pt(x: 4))
    println(f"{p.length()}")
    println(f"{total(Bag<int>(5))}")
"#,
        "1\n5",
    );
}

/// A generic body nothing calls builds its instance at an open argument; the
/// trait default that instance would answer with is never compiled at that
/// open argument, and the live instantiation runs its own copy.
#[test]
fn test_an_uncalled_generic_body_does_not_compile_the_shared_trait_default() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn pick(a T, b T) T
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl<U> implements Op<U>
    fn pick(a U, b U) U
        return a

class Factory<T>
    fn make() Op<T>
        return Impl<T>()

fn main()
    let f = Factory<String>()
    let o = f.make()
    println(o.keep("a" + "b", "c" + "d"))
"#,
        "cd",
    );
}

/// A closure inside a generic function builds its instance at the function's
/// open parameter in the shared body nothing calls; the call through the
/// trait reaches the copy compiled at the caller's type.
#[test]
fn test_a_closure_in_an_uncalled_shared_body_does_not_compile_the_shared_trait_default() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn pick(a T, b T) T
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl<U> implements Op<U>
    fn pick(a U, b U) U
        return a

fn wrap<T>(v T) Op<T>
    let mk = fn() Impl<T>: Impl<T>()
    return mk()

fn main()
    let o = wrap("z")
    println(o.keep("a" + "b", "c" + "d"))
"#,
        "cd",
    );
}

/// A generic factory whose parameter is inferred from its value arguments
/// builds the instance at the caller's type, and the call through the trait it
/// returns reaches that instantiation's body.
#[test]
fn test_a_generic_factory_inferred_from_its_arguments_answers_at_its_instantiation() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn keep(a T, b T) T

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

fn make<T>(a T, b T) Op<T>
    return Impl<T>()

fn main()
    let o = make("a" + "b", "c" + "d")
    println(o.keep("p" + "q", "r" + "s"))
"#,
        "rs",
    );
}

/// An instantiation at a nested generic argument gets its own vtable too:
/// the list a call through the trait keeps is counted as a list of strings.
#[test]
fn test_a_nested_generic_argument_is_reached_through_its_own_trait_receiver() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

trait Op<T>
    fn keep(a T, b T) T

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

fn main()
    let o Op<List<String>> = Impl<List<String>>()
    var a = List<String>()
    a.push("a" + "b")
    var b = List<String>()
    b.push("c" + "d")
    let kept = o.keep(a, b)
    println(kept[0])
"#,
        "cd",
    );
}

/// A generic subclass that renames the parameter it passes to its base
/// reaches the base's method and the trait default the base supplies at the
/// subclass's own argument.
#[test]
fn test_a_generic_subclass_renaming_its_base_parameter_answers_through_a_trait() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn pick(a T, b T) T
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Base<U> implements Op<U>
    fn pick(a U, b U) U
        var x U = b
        x = a
        return x

class Sub<W> extends Base<W>

fn main()
    let o Op<String> = Sub<String>()
    println(o.keep("p" + "q", "r" + "s"))
    println(o.pick("p" + "q", "r" + "s"))
"#,
        "rs\npq",
    );
}

/// Three instantiations of one class called in turn from one function each
/// answer with their own body.
#[test]
fn test_three_instantiations_interleaved_in_one_function_answer_with_their_own_bodies() {
    assert_heap_guard_output(
        r#"
use system.io

struct Pt
    x int
    y int

trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl<T> implements Op<T>

fn main()
    let s Op<String> = Impl<String>()
    let i Op<int> = Impl<int>()
    let p Op<Pt> = Impl<Pt>()
    println(s.keep("a" + "b", "c" + "d"))
    println(f"{i.keep(1, 2)}")
    let q = p.keep(Pt(x: 1, y: 2), Pt(x: 3, y: 4))
    println(f"{q.x} {q.y}")
    println(s.keep("e" + "f", "g" + "h"))
    println(f"{i.keep(3, 4)}")
"#,
        "cd\n2\n3 4\ngh\n4",
    );
}

/// A caller asking for one more level of nesting six times in a row reaches
/// six instantiations and no more: the program is finite, and compiles.
/// Run untracked: the instances are held through trait-typed bindings, whose
/// release leaves the fields behind (see the ignored release test above).
#[test]
fn test_a_class_nested_six_levels_deep_by_its_caller_compiles() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list

trait Seq<T>
    fn size() int
    fn nested() Seq<List<T>>

class Vec<T> implements Seq<T>
    items List<T>
    fn init(items List<T>)
        self.items = items
    fn size() int
        return self.items.length()
    fn nested() Seq<List<T>>
        let outer = List<List<T>>()
        outer.push(self.items)
        return Vec<List<T>>(outer)

fn main()
    let s Seq<String> = Vec<String>(List(["a" + "b"]))
    let n1 = s.nested()
    let n2 = n1.nested()
    let n3 = n2.nested()
    let n4 = n3.nested()
    let n5 = n4.nested()
    let n6 = n5.nested()
    println(f"{n6.size()}")
"#,
        "1",
    );
}

/// The program with the nesting chain and an unrelated deeply nested
/// instance, declared in two orders. Whether it compiles is a question about
/// the instances it needs, never about which declaration comes first.
const NESTING_BESIDE_A_DEEP_INSTANCE: &str = r#"
use system.io
use system.collections.list

trait Seq<T>
    fn size() int
    fn nested() Seq<List<T>>

DECLARATIONS

fn main()
    println(f"{D1<String>().go()}")
    let s Seq<String> = Vec<String>(List(["a" + "b"]))
    let n1 = s.nested()
    let n2 = n1.nested()
    let n3 = n2.nested()
    let n4 = n3.nested()
    let n5 = n4.nested()
    println(f"{n5.size()}")
"#;

const NESTING_CLASS: &str = r#"
class Vec<T> implements Seq<T>
    items List<T>
    fn init(items List<T>)
        self.items = items
    fn size() int
        return self.items.length()
    fn nested() Seq<List<T>>
        let outer = List<List<T>>()
        outer.push(self.items)
        return Vec<List<T>>(outer)
"#;

const DEEP_INSTANCE_CHAIN: &str = r#"
class D1<T>
    fn go() int
        return D2<T>().go()

class D2<T>
    fn go() int
        return D3<T>().go()

class D3<T>
    fn go() int
        return D4<T>().go()

class D4<T>
    fn go() int
        return D5<T>().go()

class D5<T>
    fn go() int
        let v = Vec<List<List<List<List<List<T>>>>>>(List<List<List<List<List<List<T>>>>>>())
        return v.size()
"#;

#[test]
fn test_a_nesting_class_declared_first_compiles_beside_a_deep_instance() {
    let declarations = format!("{NESTING_CLASS}\n{DEEP_INSTANCE_CHAIN}");
    assert_runs_untracked(
        &NESTING_BESIDE_A_DEEP_INSTANCE.replace("DECLARATIONS", &declarations),
        "0\n1",
    );
}

#[test]
fn test_a_nesting_class_declared_last_compiles_beside_a_deep_instance() {
    let declarations = format!("{DEEP_INSTANCE_CHAIN}\n{NESTING_CLASS}");
    assert_runs_untracked(
        &NESTING_BESIDE_A_DEEP_INSTANCE.replace("DECLARATIONS", &declarations),
        "0\n1",
    );
}

/// A value argument computed from the class's own (`Size + 1`) names one
/// instantiation once the body is compiled at a concrete size: the instance
/// built at `Buf<float, 2>` answers through its own vtable, whose `get` reads
/// the field at the width a `float` is stored at. The class's shared body,
/// which the instance would otherwise run, reads it as a plain word.
#[test]
fn test_an_instance_at_a_computed_value_argument_answers_with_its_own_body() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn get() T

class Buf<T, Size> implements Op<T>
    v T
    fn get() T
        return self.v
    fn grow() Op<T>
        return Buf<T, Size + 1>(v: self.v)

fn main()
    let b = Buf<float, 1>(v: 2.5)
    let g = b.grow()
    println(f"{g.get()} {b.get()}")
"#,
        "2.5 2.5",
    );
}

/// A class that builds itself at a value argument one larger inside the
/// method a trait call reads needs a new instantiation on every call, and is
/// refused once it needs more than the compiler compiles for one class.
#[test]
fn test_a_class_growing_a_value_argument_through_a_trait_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

trait Op
    fn depth(n int) int

class Buf<T, Size> implements Op
    v T
    fn depth(n int) int
        if n == 0
            return 0
        let a Op = Buf<T, Size + 1>(v: self.v)
        let b Op = Buf<T, Size + 1>(v: self.v)
        return 1 + a.depth(n - 1) + b.depth(n - 1)

fn main()
    let o Op = Buf<String, 1>(v: "hi" + "x")
    println(f"{o.depth(4)}")
"#,
        "MER_MIR_016",
        &[
            "`Buf` needs more than 256 instantiations of `Size`",
            "Buf<String, 1> → Buf<String, 2> → Buf<String, 3> → …",
        ],
    );
}

/// A method that calls itself statically on its class at two growing
/// arguments reaches twice as many instantiations each level; the program is
/// refused, and promptly, rather than specialized level by level.
#[test]
fn test_a_class_calling_itself_statically_at_two_growing_arguments_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Box<T>
    v T
    fn init(v T)
        self.v = v

class Impl<T>
    fn depth(n int) int
        if n == 0
            return 0
        let a = Impl<Wrap<T>>()
        let b = Impl<Box<T>>()
        return a.depth(n - 1) + b.depth(n - 1)

fn main()
    let o = Impl<int>()
    println(f"{o.depth(3)}")
"#,
        "MER_MIR_016",
        &[
            "nests its type argument 33 levels deep",
            "each call to `Impl.depth` builds `Impl` at a larger type: Impl<int> → Impl<",
            "the type argument grows on every call;",
        ],
    );
}

/// The same growth behind a trait the class implements but no call reads.
#[test]
fn test_a_class_implementing_a_trait_calling_itself_statically_while_growing_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

trait Op<T>
    fn depth(n int) int

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Box<T>
    v T
    fn init(v T)
        self.v = v

class Impl<T> implements Op<T>
    fn depth(n int) int
        if n == 0
            return 0
        let a = Impl<Wrap<T>>()
        let b = Impl<Box<T>>()
        return a.depth(n - 1) + b.depth(n - 1)

fn main()
    let o = Impl<int>()
    println(f"{o.depth(3)}")
"#,
        "MER_MIR_016",
        &["nests its type argument 33 levels deep"],
    );
}

/// A class's `equals` is compiled for every instantiation a set or map could
/// match it at, reached by no call; one that builds its class at growing
/// arguments is refused all the same.
#[test]
fn test_an_equals_building_its_class_at_growing_arguments_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

class Wrap<T>
    v int

class Box<T>
    v int

class Grow<T>
    fn equals(other Grow<T>) bool
        let a = Grow<Wrap<T>>()
        let b = Grow<Box<T>>()
        return true

fn main()
    let o = Grow<int>()
    println("ok")
"#,
        "MER_MIR_016",
        &[
            "nests its type argument 33 levels deep",
            "Grow<int> → Grow<",
        ],
    );
}

/// The same `equals`, reaching the grown instances through a trait.
#[test]
fn test_an_equals_dispatching_to_its_class_at_growing_arguments_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

trait Op<T>
    fn depth(n int) int

class Wrap<T>
    v int

class Box<T>
    v int

class Impl<T> implements Op<T>
    fn depth(n int) int
        return n
    fn equals(other Impl<T>) bool
        let a Op<Wrap<T>> = Impl<Wrap<T>>()
        let b Op<Box<T>> = Impl<Box<T>>()
        return a.depth(1) == b.depth(1)

fn main()
    let o = Impl<int>()
    println(f"{o.depth(3)}")
"#,
        "MER_MIR_016",
        &["nests its type argument 33 levels deep"],
    );
}

/// A class whose field holds the class itself at a wrapped argument names a
/// type nested without end; the program is refused rather than expanded
/// forever.
#[test]
fn test_a_class_with_a_field_at_its_own_wrapped_argument_is_refused() {
    assert_refused_within_budget(
        r#"
use system.io

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Node<T>
    v int
    next Node<Wrap<T>>

fn peek(n Node<int>) int
    return n.v

fn main()
    println("ok")
"#,
        "MER_MIR_016",
        &["nests its type argument 33 levels deep"],
    );
}

/// The chain a value-growth refusal shows starts at the instance the program
/// wrote, not partway through the instances it grew.
#[test]
fn test_a_value_growth_note_starts_at_the_instance_the_program_wrote() {
    assert_refused_within_budget(
        r#"
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn depth(n int) int
        if n == 0
            return 7
        return Buf<T, Size + 1>(self.v).depth(n - 1)

fn main()
    let s = "hi" + "x"
    let b = Buf<String, 1>(s)
    println(f"{b.depth(3)}")
"#,
        "MER_MIR_016",
        &["Buf<String, 1> → Buf<String, 2> → Buf<String, 3> → …"],
    );
}

/// The steps of a type-growth note are told apart even where they nest past
/// the levels a step is shown to.
#[test]
fn test_a_type_growth_note_never_shows_two_identical_steps() {
    assert_refused_within_budget(
        r#"
use system.io

struct S<T>
    v T

fn nest<T>(x T, n int) int
    if n == 0
        return 0
    return 1 + nest(S(v: x), n - 1)

fn main()
    let s = "a" + "b"
    println(f"{nest(s, 3)}")
"#,
        "MER_MIR_016",
        &["S<String> → S<S<String>> → S<S<S<String>>> → …"],
    );
}
