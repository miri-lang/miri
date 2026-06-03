// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL compilation and validation helpers for GPU tests.
//! These functions compile Miri source to WGSL and validate with `naga`.

use miri::ast::statement::StatementKind;
use miri::codegen::backend::Backend;
use miri::codegen::wgsl::{WgslBackend, WgslOptions};
use miri::mir::lowering::lower_function;
use miri::mir::ExecutionModel;
use miri::pipeline::Pipeline;

/// Compile a Miri source with a `gpu for` or `gpu fn` to WGSL and return the kernel text.
fn compile_to_wgsl(source: &str) -> String {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("frontend failed");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(
            |stmt| matches!(&stmt.node, StatementKind::FunctionDeclaration(d) if d.name == "main"),
        )
        .expect("source must contain 'fn main'");
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering failed");
    let kernel = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("expected a synthesized GpuKernel body");
    let artifact = WgslBackend
        .compile(
            &[(kernel.name.as_str(), &kernel.body)],
            &WgslOptions::default(),
        )
        .expect("WGSL backend should succeed");
    String::from_utf8(artifact.bytes).expect("WGSL output is UTF-8")
}

/// Compile to WGSL and validate with `naga`. Panics if parse or validate fails.
pub fn assert_gpu_wgsl_valid(source: &str) {
    let wgsl = compile_to_wgsl(source);
    let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|err| {
        panic!(
            "naga parse failed: {}\nWGSL:\n{}",
            err.emit_to_string(&wgsl),
            wgsl
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|err| panic!("naga validate failed: {:?}\nWGSL:\n{}", err, wgsl));
}
