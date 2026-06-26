// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_gpu_function_rejects_print_call() {
    let input = r#"
use system.io

gpu fn my_kernel()
    print("hello")
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_println_call() {
    let input = r#"
use system.io

gpu fn my_kernel()
    println("hello")
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_eprintln_call() {
    let input = r#"
use system.io

gpu fn my_kernel()
    eprintln("hello")
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_panic_call() {
    let input = r#"
use system.io

gpu fn my_kernel()
    panic("nope")
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_string_literal_argument() {
    let input = r#"
fn host_helper(message String)
    return

gpu fn my_kernel()
    host_helper("hello")
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_list_constructor() {
    let input = r#"
use system.collections.list

gpu fn my_kernel()
    let xs = List<int>()
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_list_literal_init() {
    let input = r#"
use system.collections.list

gpu fn my_kernel()
    let xs = List([1, 2, 3])
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_set_constructor() {
    let input = r#"
use system.collections.set

gpu fn my_kernel()
    let s = Set<int>()
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_map_constructor() {
    let input = r#"
use system.collections.map

gpu fn my_kernel()
    let m = Map<int, int>()
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_list_type_annotation() {
    let input = r#"
use system.collections.list

gpu fn my_kernel()
    let xs List<int> = List<int>()
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_string_typed_local() {
    let input = r#"
gpu fn my_kernel()
    let s = "hello"
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_allows_int_local() {
    let input = "
gpu fn my_kernel()
    let x = 1
    let y = x + 2
";
    type_checker_test(input);
}

#[test]
fn test_gpu_function_allows_float_local() {
    let input = "
gpu fn my_kernel()
    let x = 1.0
    let y = x * 2.0
";
    type_checker_test(input);
}

#[test]
fn test_gpu_function_allows_bool_local() {
    let input = "
gpu fn my_kernel()
    let flag = true
    let other = flag and false
";
    type_checker_test(input);
}

#[test]
fn test_gpu_function_allows_kernel_field_access() {
    let input = "
gpu fn my_kernel()
    let tx = kernel.thread_idx.x
    let bx = kernel.block_idx.x
    let bd = kernel.block_dim.x
    let gd = kernel.grid_dim.x
    let gi = kernel.global_idx.x
";
    type_checker_test(input);
}

#[test]
fn test_gpu_context_alias_still_type_checks() {
    let input = "
gpu fn my_kernel()
    let tx = gpu_context.thread_idx.x
";
    type_checker_test(input);
}

#[test]
fn test_gpu_context_alias_emits_one_deprecation_per_use() {
    let one_use = "
gpu fn my_kernel()
    let tx = gpu_context.thread_idx.x
";
    assert_eq!(count_warnings_with_code(one_use, "W0004"), 1);

    let two_uses = "
gpu fn my_kernel()
    let tx = gpu_context.thread_idx.x
    let bx = gpu_context.block_idx.x
";
    assert_eq!(count_warnings_with_code(two_uses, "W0004"), 2);
}

#[test]
fn test_kernel_identifier_emits_no_deprecation() {
    let input = "
gpu fn my_kernel()
    let tx = kernel.thread_idx.x
";
    assert_eq!(count_warnings_with_code(input, "W0004"), 0);
}

#[test]
fn test_gpu_context_outside_gpu_fn_is_not_deprecated() {
    let input = "
fn host()
    let gpu_context = 5
    let x = gpu_context + 1
";
    assert_eq!(count_warnings_with_code(input, "W0004"), 0);
}

#[test]
fn test_gpu_function_allows_calling_other_gpu_compatible_helper() {
    let input = "
fn add_one(x int) int: x + 1

gpu fn my_kernel()
    let y = add_one(2)
";
    type_checker_test(input);
}

#[test]
fn test_gpu_function_allows_fixed_array_of_int() {
    let input = "
gpu fn my_kernel()
    let xs = [1, 2, 3]
";
    type_checker_test(input);
}

#[test]
fn test_gpu_function_rejects_array_of_string() {
    let input = r#"
gpu fn my_kernel()
    let xs = ["a", "b", "c"]
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_tuple_local() {
    let input = "
gpu fn my_kernel()
    let t = (1, 2)
";
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_option_int_local() {
    let input = "
gpu fn my_kernel()
    let x int? = 5
";
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_string_arg_to_callable() {
    let input = r#"
fn echo(s String) String: s

gpu fn my_kernel()
    let r = echo("hi")
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_class_instance() {
    let input = "
class Widget
    fn init(): return

gpu fn my_kernel()
    let w = Widget()
";
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_string_param() {
    let input = r#"
gpu fn my_kernel(s String)
    let x = 1
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_list_param() {
    let input = r#"
use system.collections.list

gpu fn my_kernel(xs List<int>)
    let x = 1
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_discarded_string_return() {
    let input = r#"
fn echo() String: "x"

gpu fn my_kernel()
    echo()
"#;
    type_checker_error_test(input, "not GPU-compatible");
}

#[test]
fn test_gpu_function_rejects_method_call_on_string_literal() {
    let input = r#"
gpu fn my_kernel()
    let n = "hello".length()
"#;
    type_checker_error_test(input, "not GPU-compatible");
}
