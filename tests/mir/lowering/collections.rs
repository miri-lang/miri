// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_snapshot_contains_test, mir_snapshot_test};

#[test]
fn test_tuple_literal() {
    mir_snapshot_test(
        "fn main(): let t = (1, 2, 3)",
        r#"
            let _0: void;
            let _1: Tuple(int, int, int); // t

            bb0: {
                StorageLive(_1);
                _1 = (const Integer(I8(1)), const Integer(I8(2)), const Integer(I8(3)));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_empty_tuple() {
    mir_snapshot_test(
        "fn main(): let unit = ()",
        r#"
            let _0: void;
            let _1: Tuple(); // unit

            bb0: {
                StorageLive(_1);
                _1 = ();
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_list_literal() {
    mir_snapshot_test(
        "fn main(): let l = [1, 2, 3]",
        r#"
            let _0: void;
            let _1: Array(int, 3); // l

            bb0: {
                StorageLive(_1);
                _1 = [const Integer(I8(1)), const Integer(I8(2)), const Integer(I8(3))];
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_set_literal() {
    mir_snapshot_contains_test(
        "fn main(): let s = {1, 2, 3}",
        &[
            "// s",
            "_1 = {};",
            r#"_2 = fn miri_rt_set_add(_1, const Integer(I8(1)))"#,
            r#"_3 = fn miri_rt_set_add(_1, const Integer(I8(2)))"#,
            r#"_4 = fn miri_rt_set_add(_1, const Integer(I8(3)))"#,
        ],
    );
}

#[test]
fn test_map_literal() {
    mir_snapshot_contains_test(
        r#"fn main(): let m = {"a": 1, "b": 2}"#,
        &[
            "// m",
            "_1 = {};",
            r#"_2 = fn miri_rt_map_set(_1, const String("a"), const Integer(I8(1)))"#,
            r#"_3 = fn miri_rt_map_set(_1, const String("b"), const Integer(I8(2)))"#,
        ],
    );
}

#[test]
fn test_nested_tuple() {
    mir_snapshot_contains_test(
        "fn main(): let nested = ((1, 2), (3, 4))",
        &["// nested", "Tuple(Tuple(int, int), Tuple(int, int))"],
    );
}

#[test]
fn test_list_index_access() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let l = [10, 20, 30]
    let x = l[1]
"#,
        &["// l", "// x", "_1[_3]"],
    );
}

#[test]
fn test_tuple_index_access() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let t = (1, 2, 3)
    let x = t[0]
"#,
        &["// t", "// x", "_1.0"],
    );
}

#[test]
fn test_tuple_index_by_a_value_reads_through_an_index_projection() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let t = (1, 2, 3)
    var i = 1
    let x = t[i]
"#,
        &["// t", "// i", "// x", "_1[_2]"],
    );
}

#[test]
fn test_empty_list() {
    // Note: empty array currently lowers as Array(void, 0); type annotation not propagated
    mir_snapshot_contains_test("fn main(): let l = []", &["// l", "[]", "Array(void"]);
}

#[test]
fn test_nested_list() {
    mir_snapshot_contains_test(
        "fn main(): let l = [[1, 2], [3, 4]]",
        &["// l", "Array(Array("],
    );
}

#[test]
fn test_single_element_tuple() {
    mir_snapshot_test(
        "fn main(): let t = (42,)",
        r#"
            let _0: void;
            let _1: Tuple(int); // t

            bb0: {
                StorageLive(_1);
                _1 = (const Integer(I8(42)));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_large_tuple() {
    mir_snapshot_contains_test(
        "fn main(): let t = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)",
        &[
            "// t",
            "Tuple(int, int, int, int, int, int, int, int, int, int)",
        ],
    );
}

#[test]
fn test_large_list() {
    mir_snapshot_contains_test(
        "fn main(): let l = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]",
        &["// l", "Array(int"],
    );
}

#[test]
fn test_list_push_projected_field_types_the_field_not_the_base() {
    mir_snapshot_contains_test(
        r#"
use system.collections.list

struct Order
    amount int

fn main()
    let o = Order(100)
    var l = List<int>()
    l.push(o.amount)
"#,
        &[
            // The projected field reaches `miri_rt_list_push` as the place it is
            // read from: reference counting and the element ABI type it by the
            // field, so a plain integer is neither counted nor given a pointer
            // slot. Nothing runs between the copy-on-write check and the push,
            // which lends the list it writes, so reads it by copy.
            "_3 = fn miri_rt_list_cow(move _2)",
            "fn miri_rt_list_push(_2, _1.0)",
        ],
    );
}

#[test]
fn test_array_set_projected_field_types_the_field_not_the_base() {
    mir_snapshot_contains_test(
        r#"
struct Order
    amount int

fn main()
    let o = Order(100)
    var a = [0, 0]
    a.set(0, o.amount)
"#,
        &[
            "fn miri_rt_array_cow(move _2)",
            "let _6: int;",
            "_6 = _1.0;",
        ],
    );
}

/// Every call `body` makes to a runtime entry point, with the modes it passes
/// and the modes the entry point's declaration states.
fn runtime_call_modes(
    body: &miri::mir::Body,
) -> Vec<(String, Vec<miri::mir::ArgMode>, Vec<miri::mir::ArgMode>)> {
    use miri::mir::terminator::arg_mode;
    use miri::mir::{ArgMode, TerminatorKind};
    (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func,
                args,
                arg_modes,
                ..
            } => {
                let effect = func.called_runtime_effect()?;
                let passed = (0..args.len()).map(|at| arg_mode(arg_modes, at)).collect();
                let declared = (0..args.len())
                    .map(|at| {
                        if effect.stores(at) {
                            ArgMode::Donated
                        } else if effect.takes_over(at) {
                            ArgMode::Owned
                        } else {
                            arg_mode(arg_modes, at)
                        }
                    })
                    .collect();
                let name = func.called_runtime_name().unwrap_or_default().to_string();
                Some((name, passed, declared))
            }
            _ => None,
        })
        .collect()
}

/// A copy-on-write entry point takes its receiver over and hands back a
/// reference, so the call hands the receiver over rather than lending it.
#[test]
fn a_copy_on_write_receiver_is_handed_over() {
    let body = crate::mir::utils::mir_lower_code(
        "use system.collections.list\n\nfn main()\n    var l = List([1, 2])\n    let m = l\n    l.push(3)\n    println(f\"{m.length()}\")\n",
    );
    let calls = runtime_call_modes(&body);
    let cow = calls.iter().find(|(name, ..)| name.ends_with("_cow"));
    let (_, passed, _) = cow.unwrap_or_else(|| panic!("no copy-on-write call in\n{body}"));
    assert_eq!(passed.first(), Some(&miri::mir::ArgMode::Owned), "\n{body}");
}

/// Every runtime call lowering emits passes each argument in the mode its
/// entry point's declaration states.
#[test]
fn every_runtime_call_passes_its_declared_modes() {
    let body = crate::mir::utils::mir_lower_code(
        r#"
use system.collections.list

fn main()
    var l = List([1, 2])
    l.push(3)
    l[0] = 4
    var m = {"a": 1}
    m["b"] = 2
    var s = {1, 2}
    s.add(3)
    let t = "abc"
    let u = f"{t}{l.length()}x"
    println(f"{u}{t[0]}{m.length()}{s.length()}")
"#,
    );
    for (name, passed, declared) in runtime_call_modes(&body) {
        assert_eq!(passed, declared, "the call to {name} in\n{body}");
    }
}
