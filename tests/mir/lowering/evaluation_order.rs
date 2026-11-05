// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where an operand naming a place is read.
//!
//! An operand is read into a temporary the moment it is evaluated only when a
//! later operand of the same sequence may write it. Every other operand stays
//! the place it names, read where the call or operator runs, so ordinary code
//! pays no copy for the evaluation order.

use crate::mir::utils::mir_lower_code;
use miri::mir::{Body, Operand, Rvalue, StatementKind, TerminatorKind};

/// The arguments of every call in `body` whose callee's symbol, as written,
/// ends in `.name`.
fn call_arguments<'b>(body: &'b Body, name: &str) -> Vec<&'b [Operand]> {
    let suffix = format!(".{name}");
    body.basic_blocks
        .iter()
        .filter_map(|block| match &block.terminator.as_ref()?.kind {
            TerminatorKind::Call { func, args, .. }
                if func
                    .called_symbol()
                    .is_some_and(|symbol| symbol.to_string().ends_with(&suffix)) =>
            {
                Some(args.as_slice())
            }
            _ => None,
        })
        .collect()
}

/// The first argument of the one call of `name` in `body`.
fn first_argument<'b>(body: &'b Body, name: &str) -> &'b Operand {
    let calls = call_arguments(body, name);
    let [arguments] = calls.as_slice() else {
        panic!(
            "expected one call of `{name}`, found {}:\n{body}",
            calls.len()
        );
    };
    arguments
        .first()
        .unwrap_or_else(|| panic!("the call of `{name}` has no argument:\n{body}"))
}

/// Whether `operand` reads a place rooted at the binding `name` itself.
fn reads_binding(body: &Body, operand: &Operand, name: &str) -> bool {
    operand
        .place()
        .is_some_and(|place| body.local_decls[place.local.0].name.as_deref() == Some(name))
}

/// The left operand of the one addition in `body`.
fn addition_left(body: &Body) -> &Operand {
    let left = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| match &statement.kind {
            StatementKind::Assign(_, Rvalue::BinaryOp(miri::mir::BinOp::Add, left, _)) => {
                Some(&**left)
            }
            _ => None,
        });
    left.unwrap_or_else(|| panic!("expected an addition:\n{body}"))
}

#[test]
fn field_arguments_of_one_struct_are_read_where_the_call_runs() {
    let body = mir_lower_code(
        r#"
struct P
    x float
    y float

fn f(a float, b float) float: a + b

fn main()
    let p = P(x: 1.0, y: 2.0)
    let r = f(p.x, p.y)
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "f"), "p"),
        "{body}"
    );
}

#[test]
fn an_operators_left_side_is_read_where_the_operator_applies() {
    let body = mir_lower_code(
        r#"
fn main()
    let a = 1
    let b = 2
    let c = 3
    let r = a + b * c
"#,
    );
    assert!(reads_binding(&body, addition_left(&body), "a"), "{body}");
}

#[test]
fn a_receiver_is_read_where_its_method_is_called() {
    let body = mir_lower_code(
        r#"
class V
    var x float
    var y float

    fn dot(o V) float: self.x * o.x + self.y * o.y

fn main()
    let v = V(x: 1.0, y: 2.0)
    let w = V(x: 3.0, y: 4.0)
    let r = v.dot(w)
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "dot"), "v"),
        "{body}"
    );
}

#[test]
fn a_field_argument_is_read_where_the_call_runs_when_a_later_call_takes_values() {
    let body = mir_lower_code(
        r#"
struct V
    x float
    y float

struct Sphere
    center V
    radius float

fn normalize(v V) V: V(x: v.x, y: v.y)

fn hit(c V, d V) float: c.x + d.x

fn main()
    let s = Sphere(center: V(x: 0.0, y: 0.0), radius: 1.0)
    let d = V(x: 1.0, y: 0.0)
    let r = hit(s.center, normalize(d))
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "hit"), "s"),
        "{body}"
    );
}

#[test]
fn an_argument_a_later_argument_writes_is_read_into_a_temporary() {
    let body = mir_lower_code(
        r#"
fn bumpi(n out int) int
    n = 99
    return 0

fn showi(a int, b int) int: a + b

fn main()
    var n = 1
    let r = showi(n, bumpi(n))
"#,
    );
    assert!(
        is_held_copy_of(&body, first_argument(&body, "showi"), "n"),
        "{body}"
    );
}

/// Whether `operand` is a temporary a statement of `body` copied a place
/// rooted at the binding `name` into.
fn is_held_copy_of(body: &Body, operand: &Operand, name: &str) -> bool {
    if reads_binding(body, operand, name) {
        return false;
    }
    let Some(temporary) = operand.place() else {
        return false;
    };
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| match &statement.kind {
            StatementKind::Assign(target, Rvalue::Use(Operand::Copy(source))) => {
                target.local == temporary.local
                    && body.local_decls[source.local.0].name.as_deref() == Some(name)
            }
            _ => false,
        })
}

/// `Box`, a class whose `set` writes the object, and the functions the
/// programs below call: `bumpi` sets its `out` argument and `showi` reads
/// its first.
const PRELUDE: &str = r#"
use system.math

class Box
    var v int
    fn set(x int) int
        self.v = x
        return 0
    fn show(_a int) int: self.v

fn bumpi(n out int) int
    n = 99
    return 0

fn showi(a int, b int) int: a + b
"#;

fn lowered_main(rest: &str) -> Body {
    mir_lower_code(&format!("{PRELUDE}\n{rest}"))
}

#[test]
fn a_class_field_is_read_where_the_call_runs_when_a_later_argument_calls_an_intrinsic() {
    let body = lowered_main(
        r#"
fn main()
    let c = Box(v: 1)
    let r = showi(c.v, max(1, 2))
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "showi"), "c"),
        "{body}"
    );
}

#[test]
fn an_argument_is_read_where_the_call_runs_when_a_later_out_argument_is_another_binding() {
    let body = lowered_main(
        r#"
fn main()
    var n = 1
    var m = 2
    let r = showi(n, bumpi(m))
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "showi"), "n"),
        "{body}"
    );
}

#[test]
fn a_class_field_is_read_where_the_call_runs_when_no_later_argument_runs_code() {
    let body = lowered_main(
        r#"
fn main()
    let c = Box(v: 1)
    let k = 2
    let r = showi(c.v, k + 1)
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "showi"), "c"),
        "{body}"
    );
}

#[test]
fn a_receiver_no_later_argument_writes_is_read_where_its_method_is_called() {
    let body = lowered_main(
        r#"
fn main()
    let b = Box(v: 1)
    var n = 1
    let r = b.show(bumpi(n))
"#,
    );
    assert!(
        reads_binding(&body, first_argument(&body, "show"), "b"),
        "{body}"
    );
}

#[test]
fn a_class_field_a_later_method_call_may_write_is_read_into_a_temporary() {
    let body = lowered_main(
        r#"
fn main()
    let c = Box(v: 1)
    let r = showi(c.v, c.set(50))
"#,
    );
    assert!(
        is_held_copy_of(&body, first_argument(&body, "showi"), "c"),
        "{body}"
    );
}

#[test]
fn an_argument_a_later_assignment_writes_is_read_into_a_temporary() {
    let body = lowered_main(
        r#"
fn main()
    var n = 1
    let r = showi(n, (n = 5))
"#,
    );
    assert!(
        is_held_copy_of(&body, first_argument(&body, "showi"), "n"),
        "{body}"
    );
}

/// The block whose terminator calls the runtime function `name`, and the
/// first argument it passes.
fn runtime_call<'b>(body: &'b Body, name: &str) -> (usize, &'b Operand) {
    let call = body
        .basic_blocks
        .iter()
        .enumerate()
        .find_map(|(index, block)| match &block.terminator.as_ref()?.kind {
            TerminatorKind::Call { func, args, .. }
                if func
                    .called_symbol()
                    .is_some_and(|symbol| symbol.to_string() == name) =>
            {
                Some((index, args.first()?))
            }
            _ => None,
        });
    call.unwrap_or_else(|| panic!("expected a call of `{name}`:\n{body}"))
}

/// Whether control leaving `from` may come back to `to`.
fn reaches(body: &Body, from: usize, to: usize) -> bool {
    let mut pending = miri::mir::analysis::traversal::successors_of(body, from);
    let mut seen = std::collections::HashSet::new();
    while let Some(block) = pending.pop() {
        if block == to {
            return true;
        }
        if seen.insert(block) {
            pending.extend(miri::mir::analysis::traversal::successors_of(body, block));
        }
    }
    false
}

/// A map indexed inside a loop by a key a later operand computes is read
/// into a snapshot every iteration; each snapshot ends in the iteration that
/// took it, before the loop comes back to take the next, so ten thousand
/// iterations hold one at a time rather than ten thousand until the scope
/// the loop sits in ends.
#[test]
fn an_indexed_maps_snapshot_ends_in_the_iteration_that_took_it() {
    let body = mir_lower_code(
        r#"
use system.collections.map

fn reset(m out Map<int, String>) int
    m = {1: "new" + "a", 2: "new" + "b"}
    return 1

fn main()
    var m = {1: "old" + "a", 2: "old" + "b"}
    var total = 0
    for _ in 0..10000
        total = total + f"{m[reset(m)]}".length()
"#,
    );
    let (taking, snapshot) = runtime_call(&body, "miri_rt_map_get_checked");
    let snapshot = snapshot
        .place()
        .unwrap_or_else(|| panic!("the map read takes no place:\n{body}"))
        .local;
    let ends: Vec<usize> = (body.basic_blocks.iter().enumerate())
        .filter(|(_, block)| {
            block.statements.iter().any(|statement| {
                matches!(&statement.kind, StatementKind::StorageDead(place)
                    if place.local == snapshot && place.projection.is_empty())
            })
        })
        .map(|(index, _)| index)
        .collect();
    assert!(!ends.is_empty(), "the snapshot never ends:\n{body}");
    assert!(
        ends.iter().all(|&end| reaches(&body, end, taking)),
        "a snapshot ends only after the loop:\n{body}"
    );
}

/// The symbol of every function `body` calls, in block order.
fn callees(body: &Body) -> Vec<String> {
    (body.basic_blocks.iter())
        .filter_map(|block| match &block.terminator.as_ref()?.kind {
            TerminatorKind::Call { func, .. } => Some(func.called_symbol()?.to_string()),
            _ => None,
        })
        .collect()
}

/// `x in xs` on a list, an array or a string is the written call
/// `xs.contains(x)`: both reach the same callee, whether the element is
/// counted or not.
#[test]
fn membership_and_its_written_contains_call_reach_the_same_callee() {
    for (setup, element) in [
        ("let xs = List([1, 2, 3])", "2"),
        (r#"let xs = List(["a" + "b", "c"])"#, r#""a" + "b""#),
        ("let xs = [1, 2, 3]", "2"),
        (r#"let xs = "abc""#, r#""b""#),
    ] {
        let lower = |test: &str| {
            callees(&mir_lower_code(&format!(
                "use system.collections.list\n\nfn main()\n    {setup}\n    let x = {element}\n    let found = {test}\n"
            )))
        };
        let membership = lower("x in xs");
        assert!(
            !membership.is_empty(),
            "`x in xs` calls nothing after `{setup}`"
        );
        assert_eq!(
            membership.last(),
            lower("xs.contains(x)").last(),
            "`x in xs` and `xs.contains(x)` reach different callees after `{setup}`"
        );
    }
}
