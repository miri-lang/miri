// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that every generic class instance lowering compiles a
//! body for, and that runs a drop hook, holds one or takes part in virtual
//! dispatch, is one the type checker decided ownership over.

use miri::mir::verify::{verify_reached_agreement, AgreementGap};
use miri::pipeline::{Pipeline, PipelineResult};

/// A handle with a drop hook, a trait implemented by a generic box, and a
/// generic function building the box at whatever it is handed.
const DECLARATIONS: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

struct TraitShelf
    slot Shelving?

trait Shelving
    fn label() int

class Box<T> implements Shelving
    public var item T
    fn init(item T)
        self.item = item
    fn label() int
        return 1

fn keep_shelving(s Shelving)
    if true
        let _shelf = TraitShelf(slot: Some(s))
        println("kept")
    println("leaving keep")

fn shelve_any<T>(x T) Shelving
    return Box<T>(x)

class P<A, B>
    public var a A
    public var b B
    fn init(a A, b B)
        self.a = a
        self.b = b

fn pr<A, B>(a A, b B) P<A, B>
    return P<A, B>(a, b)
"#;

/// A balanced tree of `pr` calls `depth` levels deep whose leftmost leaf is a
/// handle and every other leaf an integer.
fn tree(depth: usize, leftmost: bool) -> String {
    match (depth, leftmost) {
        (0, true) => "Handle(5)".to_string(),
        (0, false) => "1".to_string(),
        (_, _) => format!(
            "pr({}, {})",
            tree(depth - 1, leftmost),
            tree(depth - 1, false)
        ),
    }
}

/// The program keeping the box built around `value` behind its trait
/// `keeps` times.
fn keeping(value: &str, keeps: usize) -> String {
    let keep_calls = "    keep_shelving(s)\n".repeat(keeps);
    format!(
        "{DECLARATIONS}\nfn main()\n    let t = {value}\n    let s = shelve_any(t)\n{keep_calls}    println(\"end\")\n"
    )
}

/// The pipeline's state once every body of `source` is lowered.
fn lowered(source: &str) -> PipelineResult {
    Pipeline::new()
        .get_lowered_result(source)
        .expect("the fixture lowers")
}

/// What the rule reports for `result` against the set its type checker
/// decided ownership over.
fn gaps_in(result: &PipelineResult) -> Vec<AgreementGap> {
    verify_reached_agreement(
        result.type_checker.type_definitions(),
        result.type_checker.ownership_instantiations(),
        &result.reached_instances,
    )
}

/// A tree of 127 types built in a generic function and kept behind a trait
/// is followed like a small one: the box lowering compiles around it is one
/// the type checker decided ownership over, so nothing about it is reported.
#[test]
fn an_instance_around_a_large_type_is_not_reported() {
    let result = lowered(&keeping(&tree(6, true), 1));
    assert!(
        (result.type_checker.ownership_instantiations()).contains_key("Box"),
        "the type checker decided ownership over the box"
    );
    let gaps = gaps_in(&result);
    assert!(
        !gaps.iter().any(|gap| gap.class == "Box"),
        "the box around the large tree is not reported: {gaps:?}"
    );
}

/// A box built around a handle in a generic function is an instance both the
/// type checker and lowering reach, so the program is reported for nothing.
#[test]
fn instances_both_walks_reach_are_not_reported() {
    let result = lowered(&keeping("Handle(1)", 1));
    assert!(
        (result.reached_instances.iter()).any(|instance| instance.class == "Box"),
        "lowering records the box it compiled"
    );
    let gaps = gaps_in(&result);
    assert!(gaps.is_empty(), "nothing is reported: {gaps:?}");
}

/// Removing an instance from the decided set makes the rule report it: the
/// rule is what notices a set too small.
#[test]
fn an_instance_missing_from_the_decided_set_is_reported() {
    let result = lowered(&keeping("Handle(1)", 1));
    let mut decided = result.type_checker.ownership_instantiations().clone();
    assert!(
        decided.remove("Box").is_some(),
        "the type checker decided ownership over the box"
    );
    let gaps = verify_reached_agreement(
        result.type_checker.type_definitions(),
        &decided,
        &result.reached_instances,
    );
    assert!(
        gaps.iter().any(|gap| gap.class == "Box"),
        "the box missing from the set is reported: {gaps:?}"
    );
}

/// A chain of classes whose `extends` clauses restate their parameters, a
/// generic function building the lowest of them, and a class that declares
/// no parameters of its own extending one at a written type.
const BASES: &str = r#"
use system.collections.list

class Top<T>
    public var item T
    fn init(item T)
        self.item = item
    fn take() int
        return 1

class Mid<U> extends Top<[U]>
    fn init(item [U])
        super.init(item)

class Low<V> extends Mid<V>
    fn init(item [V])
        super.init(item)

class Pinned extends Mid<Handle>
    fn init(item [Handle])
        super.init(item)

fn low_of<V>(x V) Low<V>
    return Low<V>(List([x]))
"#;

/// The instances of `class` the set `result`'s type checker decided
/// ownership over holds, each spelled by its arguments.
fn decided_instances_of(result: &PipelineResult, class: &str) -> Vec<String> {
    let decided = result.type_checker.ownership_instantiations();
    (decided.get(class).into_iter().flatten())
        .map(|arguments| {
            let spelled: Vec<String> = arguments.iter().map(|ty| ty.kind.to_string()).collect();
            spelled.join(", ")
        })
        .collect()
}

/// A class built in a generic function is an instance of every class above
/// it, at the arguments each `extends` clause restates: `low_of` reached at
/// `int` builds `Low<int>`, so `Mid<int>` and `Top<[int]>` are decided over
/// too, and lowering, which compiles the inherited bodies at those, is
/// reported for neither.
#[test]
fn a_base_reached_through_a_built_class_is_not_reported() {
    let source = format!(
        "{DECLARATIONS}\n{BASES}\nfn main()\n    let l = low_of(3)\n    println(f\"{{l.take()}}\")\n"
    );
    let result = lowered(&source);
    let gaps = gaps_in(&result);
    assert!(
        !gaps
            .iter()
            .any(|gap| gap.class == "Top" || gap.class == "Mid"),
        "no base on the chain is reported: {gaps:?}"
    );
    assert!(
        decided_instances_of(&result, "Top").contains(&"List(int)".to_string()),
        "the top of the chain is decided over: {:?}",
        decided_instances_of(&result, "Top")
    );
}

/// A class declaring no parameters reaches its base at the arguments its
/// own `extends` clause writes, and every class above that one: `Pinned`
/// reaches `Mid<Handle>` and `Top<[Handle]>`, though nothing builds either.
#[test]
fn a_base_a_class_without_parameters_extends_is_not_reported() {
    let source = format!(
        "{DECLARATIONS}\n{BASES}\nfn main()\n    let p = Pinned(List([Handle(1)]))\n    println(f\"{{p.take()}}\")\n"
    );
    let result = lowered(&source);
    let gaps = gaps_in(&result);
    assert!(
        !gaps
            .iter()
            .any(|gap| gap.class == "Top" || gap.class == "Mid"),
        "no base on the chain is reported: {gaps:?}"
    );
    assert!(
        decided_instances_of(&result, "Mid").contains(&"Handle".to_string()),
        "the base the clause writes is decided over: {:?}",
        decided_instances_of(&result, "Mid")
    );
}

/// What the rule reports for `source` about instances of `class`.
fn gaps_naming(source: &str, class: &str) -> Vec<AgreementGap> {
    let result = lowered(source);
    (gaps_in(&result).into_iter())
        .filter(|gap| gap.class == class)
        .collect()
}

/// A class declaring no parameters is compiled with every trait default its
/// `implements` clauses supply, called or not: `Digits` runs the `Indexed`
/// default `iterator`, which builds `IndexIterator<int, Digits>`, so that
/// instance is decided over though nothing calls `iterator`.
#[test]
fn an_instance_a_default_of_a_class_without_parameters_builds_is_not_reported() {
    let source = r#"
use system.ops

class Digits implements Indexed<int>
    fn length() int
        return 3
    fn element_at(index int) int
        return index

fn main()
    let d = Digits()
    println(f"{d.length()}")
"#;
    let gaps = gaps_naming(source, "IndexIterator");
    assert!(
        gaps.is_empty(),
        "no iterator instance is reported: {gaps:?}"
    );
}

/// A program declaring nothing is reported for nothing: the library bodies
/// lowering compiles for every program, the trait defaults of the library's
/// own classes and the methods of the classes declaring no parameters, are
/// followed like the program's.
#[test]
fn a_program_declaring_nothing_is_not_reported() {
    let gaps = gaps_in(&lowered("fn main()\n    let _x = 1\n"));
    assert!(gaps.is_empty(), "nothing is reported: {gaps:?}");
}

/// An array literal holds an array instance no construction names, and
/// lowering compiles that instance's `equals`: it is decided over.
#[test]
fn an_array_literal_is_not_reported() {
    let source = "fn main()\n    let a = [1, 2, 3]\n    println(f\"{a[0]}\")\n";
    let gaps = gaps_naming(source, "Array");
    assert!(gaps.is_empty(), "the array is not reported: {gaps:?}");
}

/// A map literal holds a map instance no construction names: it is decided
/// over.
#[test]
fn a_map_literal_is_not_reported() {
    let source = "fn main()\n    let m = {\"a\": 1}\n    println(f\"{m.length()}\")\n";
    let gaps = gaps_naming(source, "Map");
    assert!(gaps.is_empty(), "the map is not reported: {gaps:?}");
}

/// A method of a class declaring no parameters holds the list `split`
/// returns: it is decided over, though the method builds no list itself.
#[test]
fn a_list_a_call_returns_in_a_class_without_parameters_is_not_reported() {
    let source = r#"
class Words
    public var text String
    fn init(text String)
        self.text = text
    fn count() int
        return self.text.split(" ").length()

fn main()
    let w = Words("a b")
    println(f"{w.count()}")
"#;
    let gaps = gaps_naming(source, "List");
    assert!(gaps.is_empty(), "the list is not reported: {gaps:?}");
}

/// A generic function holding a `List<T>` in a typed local holds a list at
/// each type it is reached at: `List<int>` and `List<bool>` are decided over.
#[test]
fn a_list_a_generic_function_holds_is_not_reported() {
    let source = r#"
use system.collections.list

fn hold<T>(x T) int
    let items List<T> = List<T>()
    return items.length()

fn main()
    println(f"{hold(1)} {hold(true)}")
"#;
    let gaps = gaps_naming(source, "List");
    assert!(gaps.is_empty(), "no list is reported: {gaps:?}");
}

/// A trait default holds instances spelling `Self`, which every class the
/// default is copied into restates as itself: `L` runs `Labeled.go`, which
/// holds `List<Box<Self>>` and an `Array<Box<Self>, 2>` literal, so
/// `List<Box<L>>` and `Array<Box<L>, 2>` are decided over.
#[test]
fn an_instance_spelling_self_in_a_default_is_not_reported() {
    let source = r#"
use system.collections.list

trait Same
    fn same(other Self) bool
    fn pick(others [Self]) Self?
        for o in others
            if self.same(o)
                return Some(o)
        None

class Box<T> implements Same
    var t T
    var n int
    fn init(t T, n int)
        self.t = t
        self.n = n
    public fn same(other Box<T>) bool
        self.n == other.n

trait Labeled
    fn label() String
    fn go() bool
        let a = Box<Self>(self, 1)
        let r = a.pick(List([Box<Self>(self, 2), Box<Self>(self, 1)]))
        match r
            Some(v): v.t.label() == self.label() and v.n == 1
            None: false

class L implements Labeled
    var s String
    fn init(s String)
        self.s = s
    public fn label() String
        self.s

fn main()
    let l = L("q")
    println(f"{l.go()}")
"#;
    let gaps: Vec<AgreementGap> = (gaps_in(&lowered(source)).into_iter())
        .filter(|gap| gap.class == "List" || gap.class == "Array")
        .collect();
    assert!(
        gaps.is_empty(),
        "no instance around `Box<L>` is reported: {gaps:?}"
    );
}

/// A method of a value-generic class builds the class at a value argument
/// computed from its own value parameter, `Size + K`, and hands it out
/// behind a trait: `Buf<float, 1>.grow` builds `Buf<float, 2>`, whose vtable
/// `g.get()` reads, so `Buf<float, 2>` is decided over.
#[test]
fn an_instance_at_a_value_argument_computed_from_a_parameter_is_not_reported() {
    let source = r#"
use system.io

const K = 1

trait Op<T>
    fn get() T

class Buf<T, Size> implements Op<T>
    v T
    fn get() T
        return self.v
    fn grow() Op<T>
        return Buf<T, Size + K>(v: self.v)

fn main()
    let b = Buf<float, 1>(v: 2.5)
    let g = b.grow()
    println(f"{g.get()} {b.get()}")
"#;
    let gaps = gaps_naming(source, "Buf");
    assert!(
        gaps.is_empty(),
        "the instance one size up is not reported: {gaps:?}"
    );
}

/// A trait implemented by a generic class whose method builds a `Box` at the
/// class's argument, and a struct holding the trait at `int`.
const MAKER_IN_A_FIELD: &str = r#"
trait Maker<T>
    fn make() int

class Impl<T> implements Maker<T>
    public var v T
    fn init(v T)
        self.v = v
    fn make() int
        let b = Box<T>(self.v)
        return b.label()

fn as_maker<T>(v T) Maker<T>
    return Impl<T>(v)

struct MakerHolder
    m Maker<int>
"#;

/// A call through a field reads the slot at the field's declared trait
/// arguments: `holder.m` is a `Maker<int>`, which the `Impl<Handle>` built
/// for `unused` never stands behind, so lowering compiles no `make` of it and
/// no `Box<Handle>`, the instance the type checker left out.
#[test]
fn a_trait_call_through_a_field_is_not_reported() {
    let source = format!(
        "{DECLARATIONS}\n{MAKER_IN_A_FIELD}\nfn main()\n    let unused = as_maker(Handle(7))\n    let holder = MakerHolder(m: as_maker(3))\n    println(f\"{{holder.m.make()}}\")\n"
    );
    let gaps = gaps_naming(&source, "Box");
    assert!(
        gaps.is_empty(),
        "no box around a handle is reported: {gaps:?}"
    );
}

/// A program class named like a library type parameter is not read as that
/// parameter: `Map<String, K>` binds the map's `K` to `String` and its `V` to
/// the program's `K`, and the map's bodies keep that `K` a class, so lowering
/// compiles no `Map<String, String>`.
#[test]
fn a_class_named_like_a_library_parameter_is_not_reported() {
    let source = r#"
use system.collections.map

class K
    var n int

fn main()
    var m = Map<String, K>()
    m["a"] = K(n: 3)
    println(f"{m.length()}")
"#;
    let gaps = gaps_naming(source, "Map");
    assert!(gaps.is_empty(), "no map is reported: {gaps:?}");
}
