// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `gpu let` / `gpu var` lower into MIR `Local`s that carry
// `BindingResidency::Gpu`. The `Body` pretty-printer must show the `gpu`
// keyword so the residency round-trips through Display.

use miri::ast::statement::StatementKind as AstStatementKind;
use miri::mir::body::BindingResidency;
use miri::mir::lowering::lower_function;
use miri::pipeline::Pipeline;

fn get_main_mir(source: &str) -> miri::mir::Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            if let AstStatementKind::FunctionDeclaration(func) = &stmt.node {
                func.name == "main"
            } else {
                false
            }
        })
        .expect("No main function found");

    lower_function(func_stmt, &result.type_checker, false, false)
        .expect("Lowering failed")
        .0
}

#[test]
fn gpu_let_stamps_residency_on_local() {
    let body = get_main_mir(
        "
fn main()
    gpu let g = 0
",
    );

    let g_decl = body
        .local_decls
        .iter()
        .find(|d| d.name.as_deref() == Some("g"))
        .expect("Variable 'g' missing from MIR locals");

    assert_eq!(g_decl.residency, BindingResidency::Gpu);
}

#[test]
fn host_let_default_residency_is_host() {
    let body = get_main_mir(
        "
fn main()
    let h = 0
",
    );

    let h_decl = body
        .local_decls
        .iter()
        .find(|d| d.name.as_deref() == Some("h"))
        .expect("Variable 'h' missing from MIR locals");

    assert_eq!(h_decl.residency, BindingResidency::Host);
}

#[test]
fn gpu_var_pretty_print_round_trips_keyword() {
    let body = get_main_mir(
        "
fn main()
    gpu var g = 0
",
    );

    let printed = format!("{}", body);

    assert!(
        printed.contains("gpu let") || printed.contains("gpu var"),
        "Body Display must emit 'gpu' on gpu-resident locals; got:\n{}",
        printed
    );
}

/// A conditional's branch value reads the binding it names into the host value
/// the conditional produces, so a gpu binding there is read back first.
///
/// Checked on the lowered body after the readback pass rather than a run:
/// moving a binding out on one
/// branch of a conditional unbalances its reference count, host or gpu alike,
/// which the RC verifier refuses before the program could run.
#[test]
fn a_conditional_branch_value_reads_a_gpu_binding_back_first() {
    let body = get_main_mir(
        "
use system.collections.array

fn main()
    gpu var buf = [0, 0, 0, 0]
    gpu forall i in 0..4
        buf[i] = i
    let k = 5
    let h = if k > 1: buf else: [1, 1, 1, 1]
",
    );

    let mut body = body;
    miri::mir::residency::insert_readbacks(&mut body);
    let violations = miri::mir::verify::verify_cross_residency_readback(&body);
    assert!(
        violations.is_empty(),
        "the branch value must be fenced, got: {}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// `main` of `source` as lowering leaves it, then through the readback pass.
fn main_after_readback_pass(source: &str) -> miri::mir::Body {
    let mut body = get_main_mir(source);
    miri::mir::residency::insert_readbacks(&mut body);
    body
}

/// The readback calls in `body`.
fn readback_calls(body: &miri::mir::Body) -> usize {
    body.basic_blocks
        .iter()
        .filter(|block| {
            matches!(
                block.terminator.as_ref().map(|t| &t.kind),
                Some(miri::mir::TerminatorKind::Call { func, .. })
                    if func.called_symbol() == Some("miri_gpu_readback")
            )
        })
        .count()
}

fn assert_verifies_clean(body: &miri::mir::Body) {
    let violations = miri::mir::verify::verify_cross_residency_readback(body);
    assert!(
        violations.is_empty(),
        "the pass's output must verify clean, got: {}",
        violations
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    );
}

/// A read inside a loop after one launch before it: the first turn finds the
/// device ahead, every later turn finds the host array current. One readback,
/// guarded by the handle's flag, covers every turn.
#[test]
fn a_read_in_a_loop_after_a_launch_is_guarded_by_a_flag() {
    let body = main_after_readback_pass(
        "
use system.collections.array

fn main()
    gpu var g = [0, 0, 0, 0]
    forall i in 0..4
        g[i] = 1
    var s = 0
    for k in 0..200
        let c = g
        s = s + c[0]
",
    );
    assert_verifies_clean(&body);
    assert_eq!(readback_calls(&body), 1, "one readback site:\n{body}");
    assert_eq!(body.device_stale_flags.len(), 1, "one flag:\n{body}");
}

/// Reads in different positions after one launch share the readback the first
/// of them needs.
#[test]
fn reads_after_one_launch_share_one_readback() {
    let body = main_after_readback_pass(
        "
use system.collections.array

fn main()
    gpu var g = [0, 0, 0, 0]
    forall i in 0..4
        g[i] = 1
    let t = (g, 1)
    let h = g
    var s = 0
    for x in g
        s = s + x
",
    );
    assert_verifies_clean(&body);
    assert_eq!(readback_calls(&body), 1, "one readback:\n{body}");
    assert!(body.device_stale_flags.is_empty(), "no read needs a flag");
}

/// A binding no launch touched has no device buffer to read back.
#[test]
fn a_binding_no_launch_touched_is_not_read_back() {
    let body = main_after_readback_pass(
        "
use system.collections.array

fn main()
    gpu var g = [1, 2, 3, 4]
    let h = g
",
    );
    assert_verifies_clean(&body);
    assert_eq!(readback_calls(&body), 0, "no readback:\n{body}");
}

/// The calls to runtime entry `symbol` in `body`.
fn calls_to(body: &miri::mir::Body, symbol: &str) -> usize {
    body.basic_blocks
        .iter()
        .filter(|block| {
            matches!(
                block.terminator.as_ref().map(|t| &t.kind),
                Some(miri::mir::TerminatorKind::Call { func, .. })
                    if func.called_symbol() == Some(symbol)
            )
        })
        .count()
}

/// The device handle of the user binding named `name`.
fn handle_of(body: &miri::mir::Body, name: &str) -> Option<miri::mir::body::DeviceHandleId> {
    body.local_decls
        .iter()
        .find(|decl| decl.name.as_deref() == Some(name))
        .and_then(|decl| decl.device_handle)
}

/// `gpu var b = a` hands `a`'s device buffer over to a handle of `b`'s own
/// rather than sharing `a`'s: `a` may be assigned a new value afterwards, and
/// that value has to reach a buffer of `a`'s, not the one `b` now holds.
#[test]
fn a_gpu_to_gpu_move_hands_the_buffer_to_a_handle_of_its_own() {
    let body = main_after_readback_pass(
        "
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0]
    gpu var b = a
    gpu forall i in 0..4
        b[i] = i * 5
    a = [9, 9, 9, 9]
    let z = b
    let y = a
",
    );
    assert_verifies_clean(&body);
    assert_ne!(handle_of(&body, "a"), handle_of(&body, "b"), "{body}");
    assert_eq!(calls_to(&body, "miri_gpu_transfer"), 1, "{body}");
    assert_eq!(
        readback_calls(&body),
        1,
        "only `b` lags its device:\n{body}"
    );
}

/// A readback into an array the body owns gives the binding a host array of its
/// own only when another value shares it: copy-on-write, not an unconditional
/// clone that the readback then overwrites in full.
#[test]
fn a_readback_detaches_a_shared_host_array_by_copy_on_write() {
    let body = main_after_readback_pass(
        "
use system.collections.array

fn main()
    gpu var g = [0, 0, 0, 0]
    forall i in 0..4
        g[i] = 1
    let h = g
",
    );
    assert_verifies_clean(&body);
    assert_eq!(readback_calls(&body), 1, "{body}");
    assert_eq!(calls_to(&body, "miri_rt_array_clone"), 0, "{body}");
    assert_eq!(calls_to(&body, "miri_rt_array_cow"), 1, "{body}");
}
