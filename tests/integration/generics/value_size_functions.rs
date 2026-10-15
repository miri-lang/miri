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
