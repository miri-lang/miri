// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL ← Miri source-map emission.
//!
//! `compile_module` returns, alongside the WGSL text, a list of
//! `(wgsl_line, miri_offset)` spans so a website or debugger can highlight the
//! Miri source line that produced a given WGSL line.

use miri::codegen::wgsl::{compile_module, WgslOptions};
use miri::mir::ExecutionModel;
use miri::pipeline::Pipeline;

/// Convert a byte offset into a 1-based source line number.
fn line_of_offset(source: &str, offset: usize) -> u32 {
    source[..offset.min(source.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count() as u32
        + 1
}

fn compile(source: &str) -> (String, Vec<(u32, usize)>) {
    let pipeline = Pipeline::new();
    let bodies = pipeline
        .get_gpu_mir_bodies(source)
        .expect("lowering failed");

    let mut module_bodies: Vec<(&str, &_)> = bodies
        .iter()
        .filter(|(_, b)| b.execution_model == ExecutionModel::GpuDevice)
        .map(|(n, b)| (n.as_str(), b))
        .collect();
    let kernel = bodies
        .iter()
        .find(|(_, b)| b.execution_model == ExecutionModel::GpuKernel)
        .expect("expected a synthesized GpuKernel body");
    module_bodies.push((kernel.0.as_str(), &kernel.1));

    let module = compile_module(&module_bodies, &WgslOptions::default())
        .expect("wgsl backend should succeed");
    let map = module
        .source_map
        .iter()
        .map(|s| (s.wgsl_line, s.miri_offset))
        .collect();
    (module.wgsl, map)
}

#[test]
fn kernel_emits_non_empty_source_map() {
    let source = r#"
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
"#;
    let (_wgsl, map) = compile(source);
    assert!(
        !map.is_empty(),
        "a kernel with a body statement must produce at least one source-map entry"
    );
}

#[test]
fn source_map_entries_are_sorted_and_in_bounds() {
    let source = r#"
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] * 2
"#;
    let (wgsl, map) = compile(source);
    let wgsl_line_count = wgsl.lines().count() as u32;
    let source_len = source.len();

    let mut prev = 0u32;
    for (wgsl_line, miri_offset) in &map {
        assert!(*wgsl_line >= 1, "wgsl line is 1-based");
        assert!(
            *wgsl_line <= wgsl_line_count,
            "wgsl line {} out of range (module has {} lines)",
            wgsl_line,
            wgsl_line_count
        );
        assert!(
            *miri_offset < source_len,
            "miri offset {} out of source bounds ({})",
            miri_offset,
            source_len
        );
        assert!(
            *wgsl_line >= prev,
            "entries must be sorted by wgsl line ({} after {})",
            wgsl_line,
            prev
        );
        prev = *wgsl_line;
    }
}

#[test]
fn source_map_accounts_for_f16_enable_preamble() {
    // A module naming `f16` gets an `enable f16;` preamble prepended by `finish`,
    // shifting every WGSL line down. Mapped lines must stay consistent with the
    // final text and never point back into the preamble.
    let source = r#"
use system.gpu
use system.collections.array

fn main()
    gpu let a = Array<f16, 4>()
    gpu var dst = Array<f16, 4>()
    gpu forall i in 0..4
        dst[i] = a[i]
"#;
    let (wgsl, map) = compile(source);
    assert!(
        wgsl.starts_with("enable f16;"),
        "expected the f16 preamble; WGSL:\n{}",
        wgsl
    );
    assert!(
        !map.is_empty(),
        "f16 kernel must still produce a source map"
    );
    let wgsl_line_count = wgsl.lines().count() as u32;
    for (wgsl_line, _) in &map {
        assert!(
            *wgsl_line > 2,
            "no entry may point into the `enable f16;` preamble (line {})",
            wgsl_line
        );
        assert!(
            *wgsl_line <= wgsl_line_count,
            "wgsl line {} out of range after preamble shift ({} lines)",
            wgsl_line,
            wgsl_line_count
        );
    }
}

#[test]
fn source_map_points_at_the_assignment_line() {
    let source = r#"
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + 7
"#;
    let (wgsl, map) = compile(source);

    // Byte range of the body assignment in the Miri source.
    let assign_start = source.find("dst[i] = a[i]").expect("assignment present");
    let assign_line = line_of_offset(source, assign_start);

    // Some map entry must point back to the assignment's source line, and the
    // WGSL line it names must itself be an assignment (contains `=`).
    let wgsl_lines: Vec<&str> = wgsl.lines().collect();
    let hit = map.iter().find(|(wgsl_line, miri_offset)| {
        line_of_offset(source, *miri_offset) == assign_line
            && wgsl_lines
                .get((*wgsl_line as usize).saturating_sub(1))
                .is_some_and(|l| l.contains('='))
    });
    assert!(
        hit.is_some(),
        "expected a source-map entry mapping a WGSL assignment line back to Miri line {}.\nWGSL:\n{}\nmap: {:?}",
        assign_line,
        wgsl,
        map
    );
}
