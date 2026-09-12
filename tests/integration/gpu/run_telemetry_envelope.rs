// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri run --format json` reports what a program's GPU work cost.
//!
//! A reader of the envelope alone cannot otherwise tell a kernel that computed
//! zeros from a launch whose results were never read back — both print zeros
//! and exit 0. The residency counters the runtime already keeps travel in the
//! envelope's `gpu` object, which is present exactly when the run touched the
//! GPU runtime.

use crate::utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

/// Run `miri run --format json` over `source` and return the parsed envelope.
fn run_envelope(source: &str) -> serde_json::Value {
    let mut file = NamedTempFile::with_suffix(".mi").expect("temp file");
    write!(file, "{}", source).expect("write source");
    let output = miri_cmd()
        .arg("run")
        .arg(file.path())
        .arg("--format")
        .arg("json")
        .output()
        .expect("miri run");
    let stdout = String::from_utf8(output.stdout).expect("utf-8 stdout");
    serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("envelope is not JSON: {e}\n{stdout}"))
}

/// A program that never reaches the GPU runtime carries no `gpu` object: the
/// field's presence is what says the run used the GPU at all.
#[test]
fn run_envelope_omits_gpu_counts_for_a_host_only_program() {
    let envelope = run_envelope(
        "
fn main()
    println(\"host only\")
",
    );
    assert_eq!(envelope["ok"], true, "envelope: {envelope}");
    assert!(
        envelope.get("gpu").is_none(),
        "a host-only run reported GPU counters: {envelope}"
    );
}

/// A program that declared a gpu binding and launched nothing reports zeros
/// rather than nothing: the counters say what the run cost, and the object's
/// presence says it reached the GPU runtime at all. Declaring a binding needs
/// no adapter, so this holds the presence rule on any machine.
#[test]
fn run_envelope_reports_zeros_for_a_gpu_binding_that_never_launched() {
    let envelope = run_envelope(
        "
use system.collections.array

fn main()
    gpu var data = [0, 0, 0, 0]
    println(\"declared\")
",
    );
    assert_eq!(envelope["ok"], true, "envelope: {envelope}");
    let gpu = &envelope["gpu"];
    assert_eq!(gpu["uploads"], 0, "envelope: {envelope}");
    assert_eq!(gpu["launches"], 0, "envelope: {envelope}");
    assert_eq!(gpu["readbacks"], 0, "envelope: {envelope}");
}

/// A launch followed by a readback reports both, and the upload that fed the
/// kernel. These are the three numbers that separate "the kernel computed
/// zeros" from "the results were never read back".
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn run_envelope_reports_uploads_launches_and_readbacks() {
    super::device::require_gpu_int64();
    let envelope = run_envelope(
        "
use system.collections.array

fn main()
    gpu var data = [0, 0, 0, 0]
    gpu forall i in 0..4
        data[i] = i * i

    let host = data
    println(f\"{host[3]}\")
",
    );
    assert_eq!(envelope["ok"], true, "envelope: {envelope}");
    let gpu = &envelope["gpu"];
    assert_eq!(gpu["uploads"], 1, "envelope: {envelope}");
    assert_eq!(gpu["launches"], 1, "envelope: {envelope}");
    assert_eq!(gpu["readbacks"], 1, "envelope: {envelope}");
}

/// The case the counters exist for: a program that launches a kernel and never
/// reads it back reports `readbacks: 0` beside a non-zero launch count, so the
/// zeros it printed are explained rather than plausible.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn run_envelope_reports_a_launch_that_was_never_read_back() {
    super::device::require_gpu_int64();
    let envelope = run_envelope(
        "
use system.collections.array

fn main()
    gpu var data = [0, 0, 0, 0]
    gpu forall i in 0..4
        data[i] = i * i

    println(\"launched\")
",
    );
    assert_eq!(envelope["ok"], true, "envelope: {envelope}");
    let gpu = &envelope["gpu"];
    assert_eq!(gpu["launches"], 1, "envelope: {envelope}");
    assert_eq!(gpu["readbacks"], 0, "envelope: {envelope}");
}
