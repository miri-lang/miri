// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `out` arguments in device code, as the type checker judges them.
//!
//! One `fn bump(x out int)` runs on the host and on the device. In device
//! code an `out` argument is a whole local, a buffer element (`dst[i]`) or a
//! vector component (`v.x`, `pts[i].x`); every other place is refused with
//! the copy-in, store-back repair. A helper may return nothing. On the host a
//! place in a `gpu var` stays refused, by one diagnostic carrying the
//! copy-to-host repair.

use miri::error::compiler::CompilerError;
use miri::pipeline::Pipeline;

use super::utils::{type_checker_error_with_help_test, type_checker_test};

const HELPERS: &str = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn bump(x out int)
    x = x + 1

fn bumpf(x out f32)
    x = x + 1.0
";

/// Each type error the frontend reports for `source`, as its code and its
/// message, panicking if the source type-checks or fails at another stage.
fn coded_errors(source: &str) -> Vec<(String, String)> {
    match Pipeline::new().frontend(source) {
        Ok(_) => panic!("expected type errors, but the source type-checks"),
        Err(CompilerError::TypeErrors { errors, .. }) => errors
            .iter()
            .map(|e| {
                let props = e.kind.properties();
                (props.code.as_str().to_string(), e.to_string())
            })
            .collect(),
        Err(e) => panic!("expected type errors, but got: {e:?}"),
    }
}

/// The one type error `source` reports, as its code and its message.
fn single_error(source: &str) -> (String, String) {
    let errors = coded_errors(source);
    let [error] = errors.as_slice() else {
        panic!("expected exactly one error, got {errors:?}");
    };
    error.clone()
}

#[test]
fn a_whole_local_is_handed_out_in_a_pass() {
    type_checker_test(&format!(
        "{HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var t = i
        bump(t)
        dst[i] = t
"
    ));
}

#[test]
fn a_buffer_element_at_the_thread_index_is_handed_out_in_a_pass() {
    type_checker_test(&format!(
        "{HELPERS}
fn main()
    gpu var dst = [10, 20, 30, 40]
    gpu forall i in 0..4
        bump(dst[i])
"
    ));
}

#[test]
fn a_buffer_element_of_a_kernel_parameter_is_handed_out() {
    type_checker_test(&format!(
        "{HELPERS}
gpu fn step(dst out Array<int, 4>)
    bump(dst[kernel.global_idx.x])
"
    ));
}

#[test]
fn a_component_of_a_local_vector_is_handed_out_in_a_pass() {
    type_checker_test(&format!(
        "{HELPERS}
fn main()
    gpu var dst [f32; 4] = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        var v = Vec2<f32>(1.0, 2.0)
        bumpf(v.x)
        dst[i] = v.x
"
    ));
}

#[test]
fn a_component_of_a_buffer_vector_is_handed_out_in_a_pass() {
    type_checker_test(&format!(
        "{HELPERS}
fn main()
    gpu var pts = [Vec2<f32>(1.0, 2.0), Vec2<f32>(3.0, 4.0)]
    gpu forall i in 0..2
        bumpf(pts[i].x)
"
    ));
}

#[test]
fn a_helper_returning_nothing_is_called_in_a_pass() {
    type_checker_test(
        "
use system.gpu
use system.collections.array

fn noop(x float)
    let _ = x + 1.0

fn main()
    gpu var arr = [1.0]
    gpu forall i in 0..1
        arr[i] = arr[i] + 1.0
        noop(arr[i])
",
    );
}

#[test]
fn an_out_element_at_a_shared_index_is_refused_only_as_a_concurrent_write() {
    let (code, message) = single_error(&format!(
        "{HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        bump(dst[i / 2])
"
    ));
    assert!(
        message.contains("concurrent write to gpu buffer 'dst'"),
        "{code}: {message}"
    );
}

#[test]
fn an_element_of_a_gpu_let_buffer_is_refused_as_immutable() {
    let (code, message) = single_error(&format!(
        "{HELPERS}
fn main()
    gpu let dst = [0, 0, 0, 0]
    gpu var seen = [0, 0, 0, 0]
    gpu forall i in 0..4
        bump(dst[i])
        seen[i] = 1
"
    ));
    assert_eq!(code, "MER_TYP_042", "{message}");
    assert!(message.contains("'dst' is immutable"), "{message}");
}

#[test]
fn an_element_of_a_component_is_refused_in_device_code_with_the_copy_repair() {
    let (code, message) = single_error(&format!(
        "{HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var grid = [[1, 2], [3, 4]]
        bump(grid[0][1])
        dst[i] = grid[0][1]
"
    ));
    assert_eq!(code, "MER_TYP_054", "{message}");
    assert!(
        message.contains("is handed 'grid[0][1]' in device code"),
        "{message}"
    );
}

#[test]
fn a_host_call_handed_a_place_in_a_gpu_var_reports_only_the_copy_to_host_repair() {
    let (code, message) = single_error(&format!(
        "{HELPERS}
fn main()
    gpu var dst = [0, 0, 0, 0]
    bump(dst[1])
"
    ));
    assert_eq!(code, "MER_TYP_054", "{message}");
    assert!(
        message.contains("a place in the gpu-resident 'dst'"),
        "{message}"
    );
}

const ATOMIC_ELEMENT_HANDED_OUT: &str = "
use system.gpu
use system.gpu.atomic
use system.collections.array

fn bumpu(x out u32)
    x = x + 1

fn main()
    gpu var iv = Array<Atomic<u32>, 4>()
    gpu forall i in 0..1024
        bumpu(iv[i / 256])
";

#[test]
fn an_atomic_element_is_never_handed_out() {
    let (code, message) = single_error(ATOMIC_ELEMENT_HANDED_OUT);
    assert_eq!(code, "MER_TYP_054", "{message}");
    assert!(
        message.contains("'out' parameter 'x' is handed 'iv[i]', an element of the atomic buffer 'iv', which no call takes"),
        "{message}"
    );
}

#[test]
fn an_atomic_element_handed_out_is_repaired_with_an_atomic_builtin() {
    type_checker_error_with_help_test(
        ATOMIC_ELEMENT_HANDED_OUT,
        "an element of the atomic buffer 'iv'",
        "'atomic_add(iv, i, 1)', or another 'atomic_' operation",
    );
}

#[test]
fn a_host_call_handed_a_place_in_a_gpu_var_keeps_the_read_of_another_gpu_var() {
    let errors = coded_errors(
        "
use system.gpu
use system.collections.array

fn put(x out int)
    x = 1

fn main()
    gpu var h = [0, 0, 0, 0]
    gpu var g = [0, 1, 2, 3]
    put(h[g[1]])
",
    );
    let codes: Vec<&str> = errors.iter().map(|(code, _)| code.as_str()).collect();
    assert_eq!(codes, ["MER_TAR_006", "MER_TYP_054"], "{errors:?}");
    assert!(
        errors[0].1.contains("gpu-resident 'g'"),
        "the read of g stays: {errors:?}"
    );
}

#[test]
fn an_out_parameter_of_a_launch_context_type_is_refused_in_device_code() {
    let (code, message) = single_error(
        "
use system.gpu
use system.collections.array

fn grow(d out Dim3)
    d = d

gpu fn step(dst out Array<int, 4>)
    var d = kernel.block_dim
    grow(d)
    dst[kernel.global_idx.x] = 1
",
    );
    assert_eq!(code, "MER_TYP_054", "{message}");
    assert!(
        message.contains("'out' parameter 'd' of 'grow' has type 'Dim3'"),
        "{message}"
    );
}

#[test]
fn an_element_of_a_bool_buffer_handed_out_reports_only_the_element_type() {
    let errors = coded_errors(
        "
use system.gpu
use system.collections.array

fn flip(b out bool)
    b = b == false

fn main()
    gpu var fl = Array<bool, 4>()
    gpu forall i in 0..4
        flip(fl[i])
",
    );
    assert!(
        errors.iter().any(|(code, message)| code == "MER_TAR_005"
            && message.contains(
                "has element type 'bool' which is not a valid WGSL storage-buffer element"
            )),
        "expected the element type refused among {errors:?}"
    );
    assert!(
        !errors.iter().any(|(code, _)| code == "MER_TYP_054"),
        "the place handed out gets no second refusal: {errors:?}"
    );
}

#[test]
fn a_helper_with_a_struct_out_parameter_is_refused_in_device_code() {
    let errors = coded_errors(
        "
use system.gpu
use system.collections.array

struct Pair
    a int
    b int

fn swap(p out Pair)
    p = Pair(a: p.b, b: p.a)

gpu fn step(dst out Array<int, 4>)
    var p = Pair(a: 1, b: 2)
    swap(p)
    dst[kernel.global_idx.x] = p.a
",
    );
    assert!(
        errors.iter().any(|(code, message)| code == "MER_TAR_008"
            && message.contains(
                "Function 'swap' parameter 'p' has type 'Pair' which is not GPU-compatible"
            )),
        "expected the callee's signature refused among {errors:?}"
    );
}

#[test]
fn a_helper_with_a_list_out_parameter_is_refused_in_device_code() {
    let errors = coded_errors(
        "
use system.gpu
use system.collections.array
use system.collections.list

fn fill(xs out List<int>)
    xs = List<int>()

gpu fn step(dst out Array<int, 4>)
    var xs = List<int>()
    fill(xs)
    dst[kernel.global_idx.x] = 1
",
    );
    assert!(
        errors.iter().any(|(code, message)| code == "MER_TAR_008"
            && message.contains(
                "Function 'fill' parameter 'xs' has type 'List(int)' which is not GPU-compatible"
            )),
        "expected the callee's signature refused among {errors:?}"
    );
}
