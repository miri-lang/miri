// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A kernel launch the device refuses ends the program the way every other
// runtime fault does: the reason on stderr, a registered runtime code in the
// `miri run` envelope, and exit status 1 — never a signal.

use super::device::require_gpu_int64;
use crate::utils::miri_cmd;
use std::io::Write;
use tempfile::NamedTempFile;

/// The exit status of `miri run --format json` and the envelope it printed.
fn run_json(source: &str) -> (Option<i32>, serde_json::Value) {
    let mut file = NamedTempFile::new().expect("a temporary file");
    write!(file, "{}", source).expect("the source is written");
    let path = file.path().to_str().expect("the path is UTF-8");
    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .arg("--format")
        .arg("json")
        .output()
        .expect("miri runs");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let envelope = serde_json::from_str(&stdout).unwrap_or_else(|_| panic!("not JSON: {}", stdout));
    (output.status.code(), envelope)
}

fn assert_launch_failure(source: &str, reason: &str) {
    let (status, envelope) = run_json(source);
    assert_eq!(
        envelope["ok"], false,
        "the failed launch reported ok: {envelope}"
    );
    assert_eq!(
        envelope["diagnostics"][0]["code"], "MER_RT_013",
        "the failed launch is not reported as MER_RT_013: {envelope}"
    );
    assert_eq!(status, Some(1), "a failed launch must exit 1: {envelope}");
    let text = envelope.to_string();
    assert!(
        text.contains(reason),
        "the reason `{reason}` is missing: {envelope}"
    );
}

const OVERSIZED_GRID: &str = "
use system.gpu
use system.collections.array

gpu fn addk(a Array<f32,4>, b Array<f32,4>, c out Array<f32,4>)
    let x = 1

fn main() int:
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [5.0, 6.0, 7.0, 8.0]
    gpu var c = Array<f32,4>()
    addk(a, b, c).launch(Dim3(GRID, 1, 1), Dim3(1, 1, 1))
    return 0
";

/// A grid wider than the device allows is refused with its reason and a
/// clean exit.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn oversized_grid_exits_cleanly_with_a_runtime_code() {
    require_gpu_int64();
    assert_launch_failure(
        &OVERSIZED_GRID.replace("GRID", "100000"),
        "grid dimensions exceed device limits",
    );
}

/// A grid dimension past `u32::MAX` is refused rather than wrapped to a
/// different, possibly launchable, grid: `4294967297` would otherwise wrap to
/// a one-workgroup launch that silently succeeds.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn grid_dimension_past_u32_is_refused_not_wrapped() {
    require_gpu_int64();
    assert_launch_failure(
        &OVERSIZED_GRID.replace("GRID", "4294967297"),
        "grid dimensions exceed device limits",
    );
    assert_launch_failure(
        &OVERSIZED_GRID.replace("GRID", "5000000000"),
        "grid dimensions exceed device limits",
    );
}

/// A captured scalar known only at run time that does not fit its 32-bit
/// device lane is refused before the launch through the same clean exit: the
/// value on stderr, MER_RT_013 in the envelope, exit status 1.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn out_of_range_capture_exits_cleanly_with_a_runtime_code() {
    require_gpu_int64();
    assert_launch_failure(
        "
use system.gpu

fn big() int
    return 5000000000

fn main()
    gpu var buf = [0, 0, 0, 0]
    let k = big()
    gpu forall i in 0..4
        buf[i] = k
    let host = buf
    println(f\"{host[3]}\")
",
        "exceeds i32 range",
    );
}
