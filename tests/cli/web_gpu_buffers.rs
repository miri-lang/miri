// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How `miri build --target web-gpu` decides which buffer each kernel binds and
//! what that buffer holds.
//!
//! The browser runs the kernels and nothing else: no host statement executes,
//! so every buffer's contents must be known when the bundle is built, and two
//! bindings are the same buffer only when they are the same device buffer —
//! never because two declarations happen to share a name.

use crate::utils::miri_cmd;
use serde_json::Value;
use std::io::Write;
use std::path::Path;
use tempfile::{NamedTempFile, TempDir};

/// A finished `web-gpu` build: the bundle directory's owner plus the command's
/// outcome.
struct WebBuild {
    _out_dir: TempDir,
    bundle_dir: std::path::PathBuf,
    output: std::process::Output,
}

impl WebBuild {
    fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.output.stderr).into_owned()
    }

    fn manifest(&self) -> Value {
        assert!(
            self.output.status.success(),
            "web-gpu build failed:\n{}",
            self.stderr()
        );
        let text = std::fs::read_to_string(self.bundle_dir.join("bundle.json"))
            .unwrap_or_else(|err| panic!("read manifest: {err}"));
        serde_json::from_str(&text).unwrap_or_else(|err| panic!("parse manifest: {err}"))
    }
}

fn build_web(source: &str, extra_args: &[&str]) -> WebBuild {
    let mut file = NamedTempFile::new().unwrap_or_else(|err| panic!("temp source: {err}"));
    write!(file, "{source}").unwrap_or_else(|err| panic!("write source: {err}"));
    let out_dir = tempfile::tempdir().unwrap_or_else(|err| panic!("temp dir: {err}"));
    let bundle_dir = out_dir.path().join("bundle");
    let output = miri_cmd()
        .arg("build")
        .arg(file.path())
        .args(["--target", "web-gpu", "--out"])
        .arg(&bundle_dir)
        .args(extra_args)
        .output()
        .unwrap_or_else(|err| panic!("spawn miri: {err}"));
    WebBuild {
        _out_dir: out_dir,
        bundle_dir,
        output,
    }
}

fn assert_refused(build: &WebBuild, needles: &[&str]) {
    assert!(
        !build.output.status.success(),
        "web-gpu build must be refused; bundle dir exists: {}",
        Path::new(&build.bundle_dir).exists()
    );
    let stderr = build.stderr();
    for needle in needles {
        assert!(
            stderr.contains(needle),
            "stderr must contain {needle:?}; got:\n{stderr}"
        );
    }
}

fn buffers(manifest: &Value) -> &Vec<Value> {
    manifest["buffers"]
        .as_array()
        .unwrap_or_else(|| panic!("manifest.buffers must be an array: {manifest}"))
}

fn buffer<'m>(manifest: &'m Value, name: &str) -> &'m Value {
    buffers(manifest)
        .iter()
        .find(|b| b["name"] == name)
        .unwrap_or_else(|| panic!("no buffer {name:?} in {manifest}"))
}

/// The buffer names a kernel's bindings resolve to, in binding order.
fn kernel_binding_names(manifest: &Value, entry_index: usize) -> Vec<String> {
    manifest["seed"][entry_index]["bindings"]
        .as_array()
        .unwrap_or_else(|| panic!("seed[{entry_index}].bindings: {manifest}"))
        .iter()
        .map(|b| b["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

const HOST_COMPUTED_INITIALIZER: &str = r#"const N = 4

fn make(k f32) [f32; 4]
    return [k, k + 1.0, k + 2.0, k + 3.0]

gpu let a = make(10.0)
gpu var dst = [0.0, 0.0, 0.0, 0.0]

forall i in 0..N
    dst[i] = a[i] * 2.0

let host = dst
println(f'{host[0]}')
"#;

#[test]
fn web_gpu_refuses_a_buffer_whose_initializer_runs_host_code() {
    let build = build_web(HOST_COMPUTED_INITIALIZER, &[]);
    assert_refused(
        &build,
        &[
            "MER_TAR_010",
            "'a'",
            "cannot be evaluated when the bundle is built",
        ],
    );
}

const SAME_NAME_IN_TWO_FUNCTIONS: &str = r#"fn a()
    gpu var buf = [1.0, 1.0, 1.0, 1.0]
    forall i in 0..4
        buf[i] = buf[i] * 2.0

fn b()
    gpu var buf = [5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
    forall i in 0..6
        buf[i] = buf[i] * 3.0

a()
b()
"#;

#[test]
fn web_gpu_keeps_same_named_buffers_of_different_functions_apart() {
    let manifest = build_web(SAME_NAME_IN_TWO_FUNCTIONS, &[]).manifest();
    assert_eq!(
        buffers(&manifest).len(),
        2,
        "two device buffers: {manifest}"
    );

    let first = kernel_binding_names(&manifest, 0);
    let second = kernel_binding_names(&manifest, 1);
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_ne!(first, second, "each kernel binds its own buffer");

    let lengths: Vec<u64> = [&first[0], &second[0]]
        .iter()
        .map(|name| buffer(&manifest, name)["length"].as_u64().unwrap_or(0))
        .collect();
    let mut sorted = lengths.clone();
    sorted.sort_unstable();
    assert_eq!(sorted, vec![4, 6], "buffer lengths: {manifest}");

    for name in [&first[0], &second[0]] {
        let spec = buffer(&manifest, name);
        let data: Vec<f64> = spec["initialData"]
            .as_array()
            .unwrap_or_else(|| panic!("{name} must carry initialData: {manifest}"))
            .iter()
            .filter_map(Value::as_f64)
            .collect();
        let expected = if spec["length"] == 4 {
            vec![1.0; 4]
        } else {
            vec![5.0, 6.0, 7.0, 8.0, 9.0, 10.0]
        };
        assert_eq!(data, expected, "{name}: {manifest}");
    }
}

const RUNTIME_BOUND: &str = r#"use system.collections.array

fn count() int
    return 1000

fn main()
    let n = count()
    gpu var buf = Array<f32, 1000>()
    gpu forall i in 0..n
        buf[i] = 2.0
"#;

#[test]
fn web_gpu_refuses_a_forall_bound_known_only_at_run_time() {
    let build = build_web(RUNTIME_BOUND, &[]);
    assert_refused(&build, &["MER_TAR_010", "bound of this parallel loop"]);
}

const THREE_BUFFERS: &str = r#"use system.gpu

gpu let a = [1, 2, 3, 4]
gpu let b = [10, 20, 30, 40]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = a[i] + b[i]
"#;

#[test]
fn web_gpu_release_bundle_names_buffers_after_their_declarations() {
    let manifest = build_web(THREE_BUFFERS, &["--release"]).manifest();
    for (name, data) in [("a", [1, 2, 3, 4]), ("b", [10, 20, 30, 40])] {
        let spec = buffer(&manifest, name);
        assert_eq!(spec["length"], 4, "{name}: {manifest}");
        let expected: Vec<Value> = data.iter().map(|v| Value::from(*v)).collect();
        assert_eq!(spec["initialData"], Value::from(expected), "{name}");
    }
    assert_eq!(buffer(&manifest, "dst")["length"], 4);
    let mut bound = kernel_binding_names(&manifest, 0);
    bound.sort();
    assert_eq!(bound, ["a", "b", "dst"]);
}

const WIDE_INTEGER_BUFFER: &str = r#"use system.gpu
use system.collections.array

gpu var wide = Array<i64, 4>()
gpu forall i in 0..4
    wide[i] = 7
"#;

#[test]
fn web_gpu_manifest_element_type_is_the_kernel_element_type() {
    let build = build_web(WIDE_INTEGER_BUFFER, &[]);
    let manifest = build.manifest();
    let spec = buffer(&manifest, "wide");
    assert_eq!(
        spec["elemType"], "i32",
        "i64 travels in an i32 lane: {manifest}"
    );
    let wgsl = manifest["seed"][0]["wgsl"].as_str().unwrap_or_default();
    assert!(
        wgsl.contains("array<i32>"),
        "the kernel declares the same element type:\n{wgsl}"
    );
}

const F64_BUFFER: &str = r#"use system.gpu
use system.collections.array

gpu var wide = Array<f64, 4>()
gpu forall i in 0..4
    wide[i] = 1.5
"#;

#[test]
fn web_gpu_refuses_an_element_type_the_browser_cannot_hold() {
    let build = build_web(F64_BUFFER, &[]);
    assert_refused(&build, &["MER_TAR_010", "'wide'", "f64"]);
}

const KERNEL_ON_TWO_BUFFER_SETS: &str = r#"use system.gpu
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    dst[kernel.global_idx.x] = kernel.global_idx.x + 1

fn main()
    gpu var first = Array<int, 4>()
    gpu var second = Array<int, 4>()
    fill(first).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
    fill(second).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
"#;

#[test]
fn web_gpu_refuses_a_kernel_launched_on_two_sets_of_buffers() {
    let build = build_web(KERNEL_ON_TWO_BUFFER_SETS, &[]);
    assert_refused(&build, &["MER_TAR_010", "different buffers"]);
}

const KERNEL_ON_ONE_BUFFER_SET: &str = r#"use system.gpu
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    dst[kernel.global_idx.x] = kernel.global_idx.x + 1

fn main()
    gpu var only = Array<int, 4>()
    fill(only).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
"#;

#[test]
fn web_gpu_dispatches_the_literal_grid_of_a_gpu_fn_launch() {
    let manifest = build_web(KERNEL_ON_ONE_BUFFER_SET, &[]).manifest();
    assert_eq!(kernel_binding_names(&manifest, 0), ["only"]);
    assert_eq!(buffer(&manifest, "only")["length"], 4);
    assert_eq!(
        manifest["seed"][0]["workgroups"],
        Value::from(vec![4, 1, 1]),
        "the launch asked for four workgroups: {manifest}"
    );
}

const GPU_FN_RUNTIME_GRID: &str = r#"use system.gpu
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    dst[kernel.global_idx.x] = kernel.global_idx.x + 1

fn groups() int
    return 4

fn main()
    gpu var only = Array<int, 4>()
    let grid = Dim3(groups(), 1, 1)
    fill(only).launch(grid, Dim3(1, 1, 1))
"#;

#[test]
fn web_gpu_refuses_a_gpu_fn_launch_whose_grid_is_computed_at_run_time() {
    let build = build_web(GPU_FN_RUNTIME_GRID, &[]);
    assert_refused(&build, &["MER_TAR_010", "the grid of this launch"]);
}

const KERNEL_NEVER_LAUNCHED: &str = r#"use system.gpu
use system.collections.array

gpu fn fill(dst out Array<int, 4>)
    dst[kernel.global_idx.x] = kernel.global_idx.x + 1

fn main()
    gpu var only = Array<int, 4>()
"#;

#[test]
fn web_gpu_refuses_a_kernel_that_is_never_launched() {
    let build = build_web(KERNEL_NEVER_LAUNCHED, &[]);
    assert_refused(&build, &["MER_TAR_010", "never launched"]);
}

const GPU_FN_WITH_SCALAR_ARG: &str = r#"use system.gpu
use system.collections.array

gpu fn fill(dst out Array<f32, 4>, scale f32)
    dst[kernel.global_idx.x] = scale

fn main()
    gpu var dst = Array<f32, 4>()
    fill(dst, 2.0).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
"#;

/// The browser runtime binds a scalar-input uniform only for a frame pass, so
/// a launched kernel that reads a scalar argument would fail pipeline creation
/// in the browser.
#[test]
fn web_gpu_refuses_a_gpu_fn_launch_with_a_scalar_argument() {
    let build = build_web(GPU_FN_WITH_SCALAR_ARG, &[]);
    assert_refused(&build, &["MER_TAR_010", "scalar input"]);
}

const FORALL_CAPTURING_A_SCALAR: &str = r#"use system.collections.array

fn main()
    let scale = 2.0
    gpu var buf = Array<f32, 4>()
    gpu forall i in 0..4
        buf[i] = scale
"#;

/// A `forall` outside a frame pass passes a captured scalar in the same
/// scalar-input uniform, which the browser does not bind for it.
#[test]
fn web_gpu_refuses_a_forall_capturing_a_scalar() {
    let build = build_web(FORALL_CAPTURING_A_SCALAR, &[]);
    assert_refused(&build, &["MER_TAR_010", "scalar input"]);
}
