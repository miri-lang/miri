// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The record of which call arguments bind an `out` parameter, and the write
//! checks of a parallel pass that read it.
//!
//! The record is filled as each call is checked, and the pass's write
//! checks, which run once its body is checked, judge a buffer element handed
//! to an `out` parameter as a write.

use super::utils::{type_checker_errors, type_checker_result, type_checker_test};
use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::gpu_writes::OutArgumentQuery;
use miri::ast::{Statement, StatementKind};
use miri::error::compiler::CompilerError;
use miri::pipeline::Pipeline;

const CONCURRENT_WRITE: &str = "concurrent write to gpu buffer 'dst'";

/// Every call to `callee` in `stmts`, at any depth of block and expression
/// statement.
fn calls_to<'s>(stmts: &'s [Statement], callee: &str, found: &mut Vec<&'s Expression>) {
    for stmt in stmts {
        match &stmt.node {
            StatementKind::Expression(expr) => call_in_expression(expr, callee, found),
            StatementKind::Block(inner) => calls_to(inner, callee, found),
            StatementKind::FunctionDeclaration(decl) => {
                if let Some(body) = &decl.body {
                    calls_to(std::slice::from_ref(body.as_ref()), callee, found);
                }
            }
            _ => {}
        }
    }
}

fn call_in_expression<'s>(expr: &'s Expression, callee: &str, found: &mut Vec<&'s Expression>) {
    if let ExpressionKind::Call(func, _) = &expr.node {
        if matches!(&func.node, ExpressionKind::Identifier(name, _) if name == callee) {
            found.push(expr);
        }
    }
}

#[test]
fn an_argument_bound_to_an_out_parameter_is_recorded_and_a_lent_one_is_not() {
    let result = type_checker_result(
        "
fn put(x out int, step int)
    x = x + step

fn main()
    var v = [1, 2, 3]
    let step = 2
    put(v[1], step)
",
    );
    let mut calls = Vec::new();
    calls_to(&result.ast.body, "put", &mut calls);
    let [call] = calls.as_slice() else {
        panic!("expected one call to put, found {}", calls.len());
    };
    let ExpressionKind::Call(_, args) = &call.node else {
        panic!("not a call");
    };
    let outs = &result.type_checker.out_arguments;
    assert!(outs.binds_out(call, &args[0]), "the element is taken out");
    assert!(!outs.binds_out(call, &args[1]), "the step is only lent");
}

#[test]
fn a_named_argument_bound_to_an_out_parameter_is_recorded_as_its_value() {
    let result = type_checker_result(
        "
fn put(x out int, step int)
    x = x + step

fn main()
    var v = [1, 2, 3]
    put(step: 2, x: v[1])
",
    );
    let mut calls = Vec::new();
    calls_to(&result.ast.body, "put", &mut calls);
    let [call] = calls.as_slice() else {
        panic!("expected one call to put, found {}", calls.len());
    };
    let ExpressionKind::Call(_, args) = &call.node else {
        panic!("not a call");
    };
    let value = |arg: &'_ Expression| match &arg.node {
        ExpressionKind::NamedArgument(_, value) => value.as_ref().clone(),
        _ => arg.clone(),
    };
    let outs = &result.type_checker.out_arguments;
    assert!(outs.binds_out(call, &value(&args[1])), "x is taken out");
    assert!(!outs.binds_out(call, &value(&args[0])), "step is only lent");
}

const BUMP: &str = "
use system.gpu
use system.collections.array

fn bump(x out int) int
    x = x + 1
    x
";

#[test]
fn an_out_element_at_an_index_shared_by_threads_is_a_concurrent_write() {
    let errors = type_checker_errors(&format!(
        "{BUMP}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        bump(dst[i / 2])
"
    ));
    assert!(
        errors.iter().any(|e| e.contains(CONCURRENT_WRITE)),
        "expected a concurrent write among {errors:?}"
    );
}

#[test]
fn an_out_element_in_the_value_of_an_assignment_is_a_concurrent_write() {
    let errors = type_checker_errors(&format!(
        "{BUMP}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu var seen = [0, 0, 0, 0]
    gpu forall i in 0..4
        seen[i] = bump(dst[i / 2])
"
    ));
    assert!(
        errors.iter().any(|e| e.contains(CONCURRENT_WRITE)),
        "expected a concurrent write among {errors:?}"
    );
}

#[test]
fn an_out_element_at_the_thread_index_is_no_concurrent_write() {
    type_checker_test(&format!(
        "{BUMP}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        bump(dst[i])
"
    ));
}

#[test]
fn an_out_element_at_an_index_read_from_a_buffer_is_a_concurrent_write() {
    let errors = type_checker_errors(&format!(
        "{BUMP}
fn main()
    gpu let src = [3, 2, 1, 0]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        let k = src[i]
        bump(dst[k])
"
    ));
    assert!(
        errors.iter().any(|e| e.contains(CONCURRENT_WRITE)),
        "expected a concurrent write among {errors:?}"
    );
}

#[test]
fn a_whole_buffer_handed_out_by_every_thread_is_a_concurrent_write() {
    let errors = type_checker_errors(
        "
use system.gpu
use system.collections.array

fn fill(xs out Array<int, 4>) int
    xs[0] = 1
    1

fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        fill(dst)
",
    );
    assert!(
        errors.iter().any(|e| e.contains(CONCURRENT_WRITE)),
        "expected a concurrent write among {errors:?}"
    );
}

#[test]
fn an_out_element_in_a_kernel_at_a_folded_index_is_a_concurrent_write() {
    let errors = type_checker_errors(&format!(
        "{BUMP}
gpu fn step(dst out Array<int, 4>)
    bump(dst[kernel.global_idx.x / 2])
"
    ));
    assert!(
        errors.iter().any(|e| e.contains(CONCURRENT_WRITE)),
        "expected a concurrent write among {errors:?}"
    );
}

/// The one error `source` reports, rendered.
fn single_error(source: &str) -> String {
    let errors = type_checker_errors(source);
    let [error] = errors.as_slice() else {
        panic!("expected exactly one error, got {errors:?}");
    };
    error.clone()
}

/// The one error `source` reports, as its code and its message.
fn single_coded_error(source: &str) -> (String, String) {
    let errors = match Pipeline::new().frontend(source) {
        Ok(_) => panic!("expected type errors, but the source type-checks"),
        Err(CompilerError::TypeErrors { errors, .. }) => errors,
        Err(e) => panic!("expected type errors, but got: {e:?}"),
    };
    let [error] = errors.as_slice() else {
        panic!("expected exactly one error, got {errors:?}");
    };
    let code = error.kind.properties().code.as_str().to_string();
    (code, error.to_string())
}

const ACCUMULATOR: &str =
    "loop-carried accumulator 'total' makes 'forall' iterations order-dependent";

#[test]
fn a_captured_scalar_handed_out_is_a_loop_carried_accumulator() {
    let error = single_error(&format!(
        "{BUMP}
fn main()
    var total = 0
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = i
        bump(total)
"
    ));
    assert!(error.contains(ACCUMULATOR), "{error}");
}

#[test]
fn a_captured_scalar_stored_to_and_handed_out_is_one_loop_carried_accumulator() {
    let error = single_error(&format!(
        "{BUMP}
fn main()
    var total = 0
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = i
        total = total + 1
        bump(total)
"
    ));
    assert!(error.contains(ACCUMULATOR), "{error}");
}

const FRAME_RACE: &str = "'gpu frame' pass creates a data race: buffer";

/// A frame pass that writes only through an `out` argument hands the call
/// the element, which it copies in, so the pass reads the buffer it writes:
/// the one refusal is the race. Were the write unseen, the refusal would be
/// a pass that writes nothing.
#[test]
fn a_frame_pass_writing_only_through_an_out_argument_writes_a_buffer() {
    let error = single_error(&format!(
        "{BUMP}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu frame
        gpu forall i in 0..4
            bump(dst[i])
"
    ));
    assert!(
        error.contains(&format!("{FRAME_RACE} 'dst' is both read and written")),
        "{error}"
    );
}

#[test]
fn a_frame_pass_handing_out_an_element_of_a_buffer_it_also_stores_to_is_a_race() {
    let (code, message) = single_coded_error(&format!(
        "{BUMP}
fn main()
    gpu var src = [0, 0, 0, 0]
    gpu var dst = [0, 0, 0, 0]
    gpu frame
        gpu forall i in 0..4
            dst[i] = i
            bump(src[i])
            src[i] = 1
"
    ));
    assert_eq!(code, "MER_TAR_005", "{message}");
    assert!(
        message.contains(&format!("{FRAME_RACE} 'src' is both read and written")),
        "{message}"
    );
}

#[test]
fn a_call_not_yet_checked_answers_every_argument_as_handed_out() {
    let result = type_checker_result(
        "
fn put(x out int, step int)
    x = x + step

fn main()
    var v = [1, 2, 3]
    put(v[1], 2)
",
    );
    let mut calls = Vec::new();
    calls_to(&result.ast.body, "put", &mut calls);
    let [call] = calls.as_slice() else {
        panic!("expected one call to put, found {}", calls.len());
    };
    let ExpressionKind::Call(_, args) = &call.node else {
        panic!("not a call");
    };
    let unchecked = miri::type_checker::out_positions::OutArguments::default();
    assert!(
        args.iter().all(|arg| unchecked.binds_out(call, arg)),
        "a call the record has not seen must read as writing every argument"
    );
}

const OUT_HELPERS: &str = "
use system.gpu
use system.collections.array

fn bump(x out int) int
    x = x + 1
    x

fn zero(x out int)
    x = 0

fn cp(a out int, b int)
    a = b
";

/// The one concurrent-write diagnostic `source` reports for `buffer`, and no
/// other error.
fn single_concurrent_write(source: &str, buffer: &str) {
    let errors = type_checker_errors(source);
    let [error] = errors.as_slice() else {
        panic!("expected exactly one error, got {errors:?}");
    };
    assert!(
        error.contains(&format!("concurrent write to gpu buffer '{buffer}'")),
        "expected a concurrent write to '{buffer}', got {error}"
    );
}

#[test]
fn an_out_element_in_an_if_condition_at_a_shared_index_is_a_concurrent_write() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu var o = [0, 0, 0, 0]
    gpu forall i in 0..4
        if bump(dst[i / 2]) > 0: o[i] = 1
"
        ),
        "dst",
    );
}

#[test]
fn an_out_element_in_a_while_condition_at_a_shared_index_is_a_concurrent_write() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu var o = [0, 0, 0, 0]
    gpu forall i in 0..4
        while bump(dst[i / 2]) < 3
            o[i] = 1
"
        ),
        "dst",
    );
}

#[test]
fn an_out_element_in_a_for_range_at_a_shared_index_is_a_concurrent_write() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu var o = [0, 0, 0, 0]
    gpu forall i in 0..4
        for j in 0..bump(dst[i / 2])
            o[i] = j
"
        ),
        "dst",
    );
}

#[test]
fn an_out_element_in_a_kernel_if_condition_at_a_shared_index_is_a_concurrent_write() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
gpu fn step(dst out Array<int, 4>, o out Array<int, 4>)
    if bump(dst[kernel.global_idx.x / 2]) > 0: o[kernel.global_idx.x] = 1
"
        ),
        "dst",
    );
}

#[test]
fn an_out_element_in_a_returned_value_at_a_shared_index_is_a_concurrent_write() {
    let errors = type_checker_errors(&format!(
        "{OUT_HELPERS}
gpu fn step(dst out Array<int, 4>)
    return bump(dst[kernel.global_idx.x / 2])
"
    ));
    assert!(
        errors.iter().any(|e| e.contains(CONCURRENT_WRITE)),
        "expected a concurrent write among {errors:?}"
    );
}

#[test]
fn an_index_local_handed_out_is_no_longer_one_per_thread() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var j = i
        zero(j)
        dst[j] = i
"
        ),
        "dst",
    );
}

#[test]
fn an_index_local_handed_out_beside_a_buffer_read_is_no_longer_one_per_thread() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu let src = [0, 0, 0, 0]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var j = i
        cp(j, src[i])
        dst[j] = 1
"
        ),
        "dst",
    );
}

#[test]
fn an_index_local_handed_out_in_a_kernel_is_no_longer_one_per_thread() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
gpu fn step(src Array<int, 4>, dst out Array<int, 4>)
    var j = kernel.global_idx.x
    cp(j, src[j])
    dst[j] = 1
"
        ),
        "dst",
    );
}

#[test]
fn an_index_local_handed_out_later_in_a_loop_is_not_one_per_thread_earlier_in_it() {
    single_concurrent_write(
        &format!(
            "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var j = i
        for k in 0..2
            dst[j] = k
            zero(j)
"
        ),
        "dst",
    );
}

#[test]
fn an_index_local_set_again_from_the_thread_index_after_an_out_call_is_one_per_thread() {
    type_checker_test(&format!(
        "{OUT_HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var j = i
        zero(j)
        j = i
        dst[j] = i
"
    ));
}
