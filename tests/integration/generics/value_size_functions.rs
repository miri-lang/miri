// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function generic over an array's size binds the size from the array it
//! is handed, or from a number written as its type argument.

use super::utils::*;

#[test]
fn test_the_size_is_inferred_from_the_argument() {
    assert_runs_with_output(
        "use system.collections.array\n\nfn first<Size>(src Array<int, Size>) int\n    return src[0]\n\nfn main()\n    var b = Array<int, 4>()\n    b[0] = 3\n    println(f\"{first(b)}\")\n",
        "3",
    );
}

/// Each size the function is called at is its own instance.
#[test]
fn test_two_sizes_are_two_instances() {
    assert_runs_with_output(
        "use system.collections.array\n\nfn first<Size>(src Array<int, Size>) int\n    return src[0]\n\nfn main()\n    var b = Array<int, 4>()\n    b[0] = 3\n    var c = Array<int, 2>()\n    c[0] = 7\n    println(f\"{first(b)} {first(c)}\")\n",
        "3 7",
    );
}

#[test]
fn test_an_out_parameter_sized_by_the_parameter_is_written_through() {
    assert_runs_with_output(
        "use system.collections.array\n\nfn fill<Size>(dst out Array<int, Size>)\n    dst[0] = 5\n\nfn main()\n    var b = Array<int, 4>()\n    fill(b)\n    println(f\"{b[0]}\")\n",
        "5",
    );
}

#[test]
fn test_a_number_written_as_the_type_argument_is_the_size() {
    assert_runs_with_output(
        "use system.collections.array\n\nfn fill<Size>(dst out Array<int, Size>)\n    dst[0] = 5\n\nfn main()\n    var b = Array<int, 4>()\n    fill<4>(b)\n    println(f\"{b[0]}\")\n",
        "5",
    );
}

#[test]
fn test_a_written_size_the_argument_does_not_have_is_refused() {
    assert_compiler_error(
        "use system.collections.array\n\nfn fill<Size>(dst out Array<int, Size>)\n    dst[0] = 5\n\nfn main()\n    var b = Array<int, 4>()\n    fill<5>(b)\n",
        "expected Array<int, 5>, got Array<int, 4>",
    );
}

/// Two arguments that bind the size to different numbers disagree.
#[test]
fn test_two_arguments_of_different_sizes_are_refused() {
    assert_compiler_error(
        "use system.collections.array\n\nfn both<Size>(a Array<int, Size>, b Array<int, Size>) int\n    return a[0] + b[0]\n\nfn main()\n    var x = Array<int, 4>()\n    var y = Array<int, 2>()\n    println(f\"{both(x, y)}\")\n",
        "expected Array<int, 4>, got Array<int, 2>",
    );
}

/// The size a function was instantiated at reads as an `int` in its body.
#[test]
fn test_the_size_reads_as_a_value_in_the_body() {
    assert_runs_with_output(
        "use system.collections.array\n\nfn total<Size>(a Array<int, Size>) int\n    var sum = 0\n    for i in 0..Size\n        sum += a[i]\n    return sum\n\nfn main()\n    var b = Array<int, 3>()\n    b[0] = 1\n    b[1] = 2\n    b[2] = 3\n    var c = Array<int, 2>()\n    c[0] = 10\n    c[1] = 20\n    println(f\"{total(b)} {total(c)}\")\n",
        "6 30",
    );
}

/// A class's value parameter reads as a value in its methods, each instance
/// its own.
#[test]
fn test_a_class_size_reads_as_a_value_in_its_methods() {
    assert_runs_with_output(
        "use system.collections.array\n\nclass Buf<T, Size>\n    data Array<T, Size>\n\n    fn count() int\n        return Size\n\nfn main()\n    let b = Buf<int, 3>(data: Array<int, 3>())\n    let c = Buf<int, 5>(data: Array<int, 5>())\n    println(f\"{b.count()} {c.count()}\")\n",
        "3 5",
    );
}

/// A type parameter has no value to read.
#[test]
fn test_a_type_parameter_read_as_a_value_is_refused() {
    assert_compiler_error(
        "fn show<T>(x T) int\n    return T\n\nfn main()\n    println(f\"{show(1)}\")\n",
        "type parameter",
    );
}
