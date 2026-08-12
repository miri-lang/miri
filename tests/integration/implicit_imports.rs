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

#[test]
fn test_unimported_list_constructor_suggests_import() {
    // A named generic collection constructor used without its module must
    // produce a unified "unknown type, consider importing" hint, not a bare
    // "Undefined type: List".
    assert_compiler_error(
        r#"
fn main()
    let x = List<int>()
        "#,
        "Consider importing 'system.collections.list'",
    );
}

#[test]
fn test_result_available_without_use() {
    // `Result` comes from the prelude, so returning and matching one needs no
    // `use system.result`.
    assert_runs_with_output(
        r#"
fn divide(a int, b int) Result<int, String>
    if b == 0
        return Result.Err("division by zero")
    return Result.Ok(a / b)
fn main()
    match divide(10, 2)
        Result.Ok(value): println(f"{value}")
        Result.Err(message): println(message)
        "#,
        "5",
    );
}

#[test]
fn test_user_enum_shadows_prelude_result() {
    // A shadowable prelude name does not reserve the name: the program's own
    // `Result` wins, variants and all, and the stdlib module is not loaded.
    assert_runs_with_output(
        r#"
enum Result
    Yes
    No
fn main()
    match Result.Yes
        Result.Yes: println("yes")
        Result.No: println("no")
        "#,
        "yes",
    );
}

#[test]
fn test_user_class_shadows_prelude_result() {
    // The declaration wins over a prelude name of a different kind: the stdlib's
    // `Result` is an enum, the program's is a class.
    assert_runs_with_output(
        r#"
class Result
    fn tag() int
        return 8
fn main()
    let r = Result()
    println(f"{r.tag()}")
        "#,
        "8",
    );
}

#[test]
fn test_user_struct_shadows_prelude_result() {
    assert_runs_with_output(
        r#"
struct Result
    value int
fn main()
    let r = Result(4)
    println(f"{r.value}")
        "#,
        "4",
    );
}

#[test]
fn test_user_trait_shadows_prelude_result() {
    assert_runs_with_output(
        r#"
trait Result
    fn tag() int
class Marker implements Result
    fn tag() int
        return 6
fn main()
    let m = Marker()
    println(f"{m.tag()}")
        "#,
        "6",
    );
}

#[test]
fn test_shadowing_a_prelude_type_leaves_the_rest_intact() {
    // Taking over one preloaded name must not disturb the rest of the preload:
    // string methods and the collection literals still resolve.
    assert_runs_with_output(
        r#"
enum Result
    Yes
fn main()
    let text = "abc"
    let items = [1, 2, 3]
    println(f"{text.length()} {items.length()}")
        "#,
        "3 3",
    );
}

#[test]
fn test_redeclaring_a_shadowed_prelude_name_errors() {
    // The shadow is spent by the first declaration; a second one is an ordinary
    // duplicate and must still be rejected.
    assert_compiler_error(
        r#"
enum Result
    Yes
enum Result
    No
fn main()
    println("unreachable")
        "#,
        "Type 'Result' is already defined",
    );
}

#[test]
fn test_type_the_stdlib_builds_on_is_not_shadowable() {
    // `Iterable` is implemented by the preloaded stdlib types, so it is loaded
    // eagerly and its name stays reserved. Only the shadowable tier can be
    // skipped in favour of a program's own declaration.
    assert_compiler_error(
        r#"
enum Iterable
    First
fn main()
    println("unreachable")
        "#,
        "Type 'Iterable' is already defined",
    );
}

#[test]
fn test_collection_backing_name_is_not_shadowable() {
    // The literal backings are preloaded for `[1, 2, 3]` to resolve, so their
    // names are reserved even though user code cannot refer to them.
    assert_compiler_error(
        r#"
class List
    fn tag() int
        return 1
        "#,
        "Type 'List' is already defined",
    );
}

#[test]
fn test_string_name_is_not_shadowable() {
    // `String` comes from the eager tier, which the rest of the stdlib is
    // written against, so it is reserved too.
    assert_compiler_error(
        r#"
class String
    fn tag() int
        return 1
        "#,
        "Type 'String' is already defined",
    );
}

#[test]
fn test_importing_the_module_that_defines_a_shadowed_type_errors() {
    // Declaring the type and importing it are contradictory instructions, so the
    // import reports the conflict instead of silently keeping the declaration.
    assert_compiler_error(
        r#"
use system.result
enum Result
    Yes
fn main()
    println("unreachable")
        "#,
        "Type 'Result' is declared in this program and also provided by 'system.result'",
    );
}

#[test]
fn test_importing_a_module_that_uses_a_shadowed_type_errors() {
    // `system.fs` is written against the stdlib `Result`. The conflict is named
    // at the import, in the user's file, rather than left to surface as
    // signature failures inside library source.
    assert_compiler_error(
        r#"
use system.fs
enum Result
    Yes
fn main()
    let fs = Fs()
        "#,
        "Type 'Result' is declared in this program and also provided by 'system.fs'",
    );
}

#[test]
fn test_unimported_array_constructor_suggests_import() {
    // The sized-array constructor takes a different parse/type path than the
    // bare collection identifier; it must surface the same import hint instead
    // of "Type 'Array(int, 3)' is not callable".
    assert_compiler_error(
        r#"
fn main()
    let x = Array<int, 3>(1, 2, 3)
        "#,
        "Consider importing 'system.collections.array'",
    );
}
