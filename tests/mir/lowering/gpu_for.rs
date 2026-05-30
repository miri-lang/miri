// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lower_code;
use miri::ast::statement::StatementKind;
use miri::mir::lowering::lower_function;
use miri::mir::{ExecutionModel, TerminatorKind};
use miri::pipeline::Pipeline;

#[test]
fn test_gpu_for_emits_gpu_launch_terminator() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let has_launch = body.basic_blocks.iter().any(|bb| {
        bb.terminator
            .as_ref()
            .is_some_and(|t| matches!(t.kind, TerminatorKind::GpuLaunch { .. }))
    });
    assert!(
        has_launch,
        "Expected TerminatorKind::GpuLaunch in MIR for `gpu for` body"
    );
}

fn synthesize_kernel_names(source: &str) -> Vec<String> {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("a function declaration");
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering");
    lambdas
        .into_iter()
        .filter(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .map(|l| l.name)
        .collect()
}

#[test]
fn test_two_gpu_for_loops_in_one_function_have_unique_kernel_names() {
    let names = synthesize_kernel_names(
        "
use system.gpu

fn main()
    gpu for i in 0..4
        let x = i
    gpu for j in 0..8
        let y = j
",
    );
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "expected unique kernel names, got {names:?}"
    );
}

#[test]
fn test_gpu_for_captures_variables_used_inside_nested_range() {
    // The capture collector must walk into ExpressionKind::Range so that
    // variables used only as inner-loop bounds (`for j in 0..n`) are still
    // counted as outer captures. Before the fix this silently dropped `n`
    // from the capture list, leaving the kernel referencing an undefined
    // local.
    let pipeline = Pipeline::new();
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        for j in 0..i
            dst[i] = a[i]
";
    let result = pipeline.frontend(source).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .unwrap();
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering");
    let kernel = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("kernel");
    let captured_names: Vec<&str> = kernel
        .body
        .local_decls
        .iter()
        .skip(1) // _0 is the return slot
        .take(kernel.body.arg_count)
        .filter_map(|d| d.name.as_deref())
        .collect();
    assert!(
        captured_names.contains(&"a"),
        "expected 'a' captured into kernel, got {captured_names:?}"
    );
    assert!(
        captured_names.contains(&"dst"),
        "expected 'dst' captured into kernel, got {captured_names:?}"
    );
}

#[test]
fn test_gpu_launch_terminator_carries_capture_args() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let args = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch { args, .. }) => Some(args.clone()),
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        !args.is_empty(),
        "GpuLaunch.args must be populated with capture operands, got empty"
    );
}

#[test]
fn test_gpu_for_rejects_scalar_capture() {
    // Scalars are `is_gpu_compatible` (so the body type-checks fine), but
    // the runtime dispatcher marshals every capture as a `MiriArray`
    // pointer. A scalar capture would be reinterpreted as a heap pointer,
    // causing UB at dispatch. The MIR lowering must surface this as a
    // diagnostic instead of silently producing miscompiled code.
    let pipeline = Pipeline::new();
    let source = "
use system.gpu
use system.collections.array

fn main()
    let scale = 7
    gpu var dst = [0, 0, 0, 0]
    gpu for i in 0..4
        dst[i] = scale
";
    let result = pipeline.frontend(source).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("a function declaration");
    let err = lower_function(func_stmt, &result.type_checker, false, false)
        .expect_err("expected lowering to reject scalar capture in `gpu for`");
    let msg = format!("{err}");
    assert!(
        msg.contains("non-buffer") && msg.contains("scale"),
        "expected diagnostic about non-buffer capture 'scale', got: {msg}"
    );
}

#[test]
fn test_gpu_for_loops_in_different_functions_have_unique_kernel_names() {
    let names_a = synthesize_kernel_names(
        "
use system.gpu

fn a()
    gpu for i in 0..4
        let x = i
",
    );
    let names_b = synthesize_kernel_names(
        "
use system.gpu

fn b()
    gpu for i in 0..4
        let x = i
",
    );
    assert_eq!(names_a.len(), 1);
    assert_eq!(names_b.len(), 1);
    assert_ne!(
        names_a[0], names_b[0],
        "kernel names collide between functions: {} vs {}",
        names_a[0], names_b[0]
    );
}
