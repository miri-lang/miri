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

/// A tree past the size the type checker follows is built in a generic
/// function and kept behind a trait twice: lowering compiles the box around
/// it, which the type checker never decided ownership over.
#[test]
fn an_instance_past_the_followed_size_is_reported() {
    let result = lowered(&keeping(&tree(6, true), 2));
    let gaps = gaps_in(&result);
    let boxed = gaps
        .iter()
        .find(|gap| gap.class == "Box")
        .unwrap_or_else(|| panic!("the box around the deep tree is reported: {gaps:?}"));
    assert!(boxed.is_resource, "the box holds the handle: {boxed:?}");
    let message = boxed.to_string();
    assert!(message.contains("ownership was decided over"), "{message}");
}

/// A box built around a handle in a generic function is an instance both the
/// type checker and lowering reach, so the program is reported for nothing
/// beyond what a program declaring nothing is: the library instances lowering
/// compiles for bodies codegen names outside any call.
#[test]
fn instances_both_walks_reach_are_not_reported() {
    let result = lowered(&keeping("Handle(1)", 1));
    assert!(
        (result.reached_instances.iter()).any(|instance| instance.class == "Box"),
        "lowering records the box it compiled"
    );
    let baseline = gaps_in(&lowered("fn main()\n    let _x = 1\n"));
    assert_eq!(gaps_in(&result), baseline);
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
