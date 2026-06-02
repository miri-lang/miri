// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_println_without_use() {
    // Criterion 1: println should work without `use system.io`
    assert_runs_with_output(
        r#"
fn main()
    println("hi")
        "#,
        "hi",
    );
}

#[test]
fn test_array_literal_length_without_use() {
    // Criterion 2: array literal + method should work without any use statement
    assert_runs_with_output(
        r#"
fn main()
    let a = [1, 2, 3]
    println(f"{a.length()}")
        "#,
        "3",
    );
}

#[test]
fn test_list_literal_without_use() {
    // Criterion 3: list literal should work without use system.collections.list
    assert_runs_with_output(
        r#"
fn main()
    let lst = [1, 2, 3]
    println(f"{lst.length()}")
        "#,
        "3",
    );
}

#[test]
fn test_map_literal_without_use() {
    // Criterion 3: map literal should work without use statement
    assert_runs_with_output(
        r#"
fn main()
    let m = {"a": 1, "b": 2}
    println(f"{m.length()}")
        "#,
        "2",
    );
}

#[test]
fn test_set_literal_without_use() {
    // Criterion 3: set literal should work without use statement
    assert_runs_with_output(
        r#"
fn main()
    let s = {1, 2, 3}
    println(f"{s.length()}")
        "#,
        "3",
    );
}

#[test]
fn test_explicit_array_name_requires_use() {
    // Criterion 5: explicitly naming Array type requires use statement
    // Use a valid constructor call so only a visibility rejection fails it.
    assert_compiler_error("fn main()\n    let a = Array<int, 3>()\n", "Array");
}

#[test]
fn test_explicit_list_name_requires_use() {
    // Explicitly naming List type requires use statement
    assert_compiler_error("fn main()\n    let a = List<int>()\n", "List");
}

#[test]
fn test_explicit_map_name_requires_use() {
    // Naming Map without `use system.collections.map` must error.
    assert_compiler_error("fn main()\n    let m = Map<String, int>()\n", "Map");
}

#[test]
fn test_explicit_set_name_requires_use() {
    // Naming Set without `use system.collections.set` must error.
    assert_compiler_error("fn main()\n    let s = Set<int>()\n", "Set");
}

#[test]
fn test_explicit_collection_name_with_use_compiles() {
    // The companion to the negative name tests: WITH the explicit `use`, naming
    // the collection type compiles and runs. This proves the negatives above
    // reject because the name is hidden (not because the construct is invalid),
    // and that an explicit collection import still works after the implicit
    // preload.
    assert_runs_with_output(
        r#"
use system.collections.list
fn main()
    var a = List<int>()
    a.push(7)
    println(f"{a.length()}")
        "#,
        "1",
    );
}

#[test]
fn test_collection_module_import_exposes_transitive_trait() {
    // Importing a collection module must still expose the transitive traits it
    // pulls in (here `queryable`'s parent `Iterable`), matching a fresh load.
    // The implicit preload marks these modules loaded, so this guards against the
    // guarded-reimport path dropping transitive visibility.
    assert_runs_with_output(
        r#"
use system.collections.queryable
class Box<T> implements Iterable<int>
    fn length() int
        return 0
    fn element_at(index int) int
        return 0
fn main()
    let b = Box<int>()
    println(f"{b.length()}")
        "#,
        "0",
    );
}

#[test]
fn test_gpu_available_requires_use() {
    // Criterion 6: is_gpu_available() still requires use system.gpu
    assert_compiler_error(
        r#"
fn main()
    println(f"{is_gpu_available()}")
        "#,
        "is_gpu_available",
    );
}

#[test]
fn test_accelerable_named_requires_use() {
    // Criterion 7: referring to Accelerable by name requires use system.accelerator
    assert_compiler_error(
        r#"
class Foo implements Accelerable
        "#,
        "Accelerable",
    );
}
