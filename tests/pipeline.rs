// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::pipeline::{program_uses_gpu, Pipeline};

fn detects_gpu(source: &str) -> bool {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend_script(source).expect("frontend");
    program_uses_gpu(result.ast.body.iter())
}

#[test]
fn program_uses_gpu_finds_gpu_for_at_top_level() {
    assert!(detects_gpu(
        "
use system.gpu
use system.collections.array

gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = i
"
    ));
}

#[test]
fn program_uses_gpu_walks_into_class_body() {
    assert!(detects_gpu(
        "
use system.gpu

class Worker
    gpu fn kernel()
        let x = 1
"
    ));
}

#[test]
fn program_uses_gpu_false_for_cpu_only_program() {
    assert!(!detects_gpu(
        "
use system.io
fn main()
    println(\"hello\")
"
    ));
}
