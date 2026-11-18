// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A body declaring type parameters of its own, a generic function or a
//! method's own `fn m<U>`, is compiled once per set of types the program
//! reaches it at. A value of such a parameter read out of a place is
//! borrowed at every copy reading it as a resource, so handing it to a
//! keeper, or keeping an `out` parameter of it, is refused with MER_OWN_008
//! at the body's line, naming the type the body is compiled at. At a type
//! that is no resource the same body runs and keeps a shared copy.

use super::super::utils::*;
use super::places_handed_to_keepers::{assert_refused, program, CODE};
use crate::utils::miri_check_json;

/// A generic class keeping each value it is handed.
const BIN: &str = r#"
class Bin<T>
    var items List<T>
    fn init()
        self.items = List<T>()
    fn put(x T)
        self.items.push(x)
    fn count() int
        return self.items.length()
"#;

/// Generic functions handing a value read out of a place to a keeper.
const FILERS: &str = r#"
fn file_first<T>(l List<T>, b Bin<T>)
    b.put(l[0])

fn file_each<T>(l List<T>, b Bin<T>)
    for x in l
        b.put(x)

fn stash<T>(c out T, fresh T, b Bin<T>)
    b.put(c)
    c = fresh
"#;

/// A class whose methods declare type parameters of their own.
const DESK: &str = r#"
class Desk
    fn init()
        return
    fn file_first<U>(l List<U>, b Bin<U>)
        b.put(l[0])
    fn stash<U>(c out U, fresh U, b Bin<U>)
        b.put(c)
        c = fresh
"#;

/// The program declaring `BIN`, then `declarations`, then `main`.
fn with_bin(declarations: &str, main: &str) -> String {
    program(&format!("{BIN}{declarations}{main}"))
}

/// The MER_OWN_008 refusals the check of `source` reports.
fn place_refusals(source: &str) -> Vec<miri::diagnostics::json::JsonDiagnostic> {
    (miri_check_json(source).diagnostics.into_iter())
        .filter(|diagnostic| diagnostic.code.as_deref() == Some(CODE))
        .collect()
}

/// Asserts `source` is refused with MER_OWN_008 at the line containing
/// `handing`, the message naming the instantiation `copy` the body is
/// compiled at, and returns the refusal.
fn assert_refused_at_copy(
    source: &str,
    handing: &str,
    copy: &str,
) -> miri::diagnostics::json::JsonDiagnostic {
    let diagnostic = assert_refused_at(source, CODE, handing);
    assert!(
        diagnostic
            .message
            .contains(&format!("compiled at `{copy}`")),
        "the refusal names the instantiation `{copy}`: {diagnostic:#?}"
    );
    diagnostic
}

#[test]
fn a_generic_function_keeping_an_element_is_refused_at_a_resource_instantiation() {
    let source = with_bin(
        FILERS,
        r#"
fn main()
    let l = List([Res(1)])
    let b = Bin<Res>()
    file_first(l, b)
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "    b.put(l[0])");
    assert!(
        diagnostic.message.contains("compiled at `Res`"),
        "the refusal names the instantiation: {diagnostic:#?}"
    );
}

#[test]
fn a_generic_function_keeping_a_loop_binding_is_refused_at_a_resource_instantiation() {
    let source = with_bin(
        FILERS,
        r#"
fn main()
    let l = List([Res(1)])
    let b = Bin<Res>()
    file_each(l, b)
"#,
    );
    let diagnostic = assert_refused_at_copy(&source, "        b.put(x)", "Res");
    let help = diagnostic.help.as_deref().unwrap_or_default();
    assert!(
        help.contains("`match l.remove_at(0)`"),
        "the refusal carries the repair: {diagnostic:#?}"
    );
}

#[test]
fn a_generic_function_keeping_an_element_runs_at_plain_instantiations() {
    assert_heap_guard_output(
        &with_bin(
            FILERS,
            r#"
fn main()
    let l = List([7])
    let b = Bin<int>()
    file_first(l, b)
    file_each(l, b)
    let s = List(["a"])
    let t = Bin<String>()
    file_first(s, t)
    println(f"{b.count()} {l[0]} {t.count()} {s[0]}")
"#,
        ),
        "2 7 1 a",
    );
}

#[test]
fn a_generic_function_keeping_its_out_parameter_is_refused_at_a_resource_instantiation() {
    assert_refused(
        &format!(
            "{BIN}{FILERS}{}",
            r#"
fn main()
    let b = Bin<Res>()
    var c = Res(1)
    stash(c, Res(2), b)
"#
        ),
        "    b.put(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn a_generic_function_keeping_its_out_parameter_runs_at_a_plain_instantiation() {
    assert_heap_guard_output(
        &with_bin(
            FILERS,
            r#"
fn main()
    let b = Bin<int>()
    var c = 1
    stash(c, 2, b)
    println(f"{c} {b.count()}")
"#,
        ),
        "2 1",
    );
}

#[test]
fn a_method_keeping_an_element_of_its_own_type_parameter_is_refused_at_a_resource() {
    let source = with_bin(
        DESK,
        r#"
fn main()
    let l = List([Res(1)])
    let b = Bin<Res>()
    Desk().file_first(l, b)
"#,
    );
    let diagnostic = assert_refused_at(&source, CODE, "        b.put(l[0])");
    assert!(
        diagnostic.message.contains("compiled at `Res`"),
        "the refusal names the instantiation: {diagnostic:#?}"
    );
}

#[test]
fn a_method_keeping_its_own_type_parameters_out_parameter_is_refused_at_a_resource() {
    assert_refused(
        &format!(
            "{BIN}{DESK}{}",
            r#"
fn main()
    let b = Bin<Res>()
    var c = Res(1)
    Desk().stash(c, Res(2), b)
"#
        ),
        "        b.put(c)",
        "'c'",
        "`replace(c, <new value>)` (use system.memory)",
    );
}

#[test]
fn a_method_keeping_values_of_its_own_type_parameter_runs_at_a_plain_instantiation() {
    assert_heap_guard_output(
        &with_bin(
            DESK,
            r#"
fn main()
    let l = List([7])
    let b = Bin<int>()
    let d = Desk()
    d.file_first(l, b)
    var c = 1
    d.stash(c, 2, b)
    println(f"{c} {b.count()}")
"#,
        ),
        "2 2",
    );
}

#[test]
fn a_generic_function_reached_at_a_resource_and_a_plain_type_is_refused_once() {
    let source = with_bin(
        FILERS,
        r#"
fn main()
    file_first(List([7]), Bin<int>())
    file_first(List([Res(1)]), Bin<Res>())
"#,
    );
    let refusals = place_refusals(&source);
    assert_eq!(refusals.len(), 1, "one refusal: {refusals:#?}");
    assert!(
        refusals[0].message.contains("compiled at `Res`"),
        "the refusal names the resource instantiation: {refusals:#?}"
    );
    assert!(
        !refusals[0].message.contains("compiled at `int`"),
        "the refusal names no plain instantiation: {refusals:#?}"
    );
}

#[test]
fn replace_runs_at_a_resource_instantiation() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    var c = Res(1)
    let old = replace(c, Res(2))
    println(f"old {old.id} now {c.id}")
"#,
        ),
        "old 1 now 2\nclosed 1\nclosed 2",
    );
}

#[test]
fn take_runs_at_a_resource_instantiation() {
    assert_heap_guard_output(
        &program(
            r#"
fn main()
    var o Res? = Some(Res(1))
    let t = o.take()
    println(f"empty {o.is_none()}")
    match t
        Some(r): println(f"took {r.id}")
        None: println("none")
    println("done")
"#,
        ),
        "empty true\ntook 1\ndone\nclosed 1",
    );
}

/// A generic class whose methods declare a type parameter of their own
/// beside the class's.
const TRAY: &str = r#"
class Tray<T>
    var items List<T>
    fn init(first T)
        self.items = List([first])
    fn file_own<U>(l List<U>, b Bin<U>)
        b.put(l[0])
    fn file_class<U>(b Bin<T>, tag U)
        b.put(self.items[0])
"#;

#[test]
fn a_generic_class_method_keeping_a_place_of_its_own_parameter_is_refused_at_a_resource() {
    let source = with_bin(
        TRAY,
        r#"
fn main()
    let t = Tray<int>(1)
    t.file_own(List([Res(1)]), Bin<Res>())
"#,
    );
    assert_refused_at_copy(&source, "        b.put(l[0])", "Res");
}

#[test]
fn a_generic_class_method_keeping_a_place_of_the_class_parameter_is_refused_at_a_resource() {
    let source = with_bin(
        TRAY,
        r#"
fn main()
    let t = Tray<Res>(Res(1))
    t.file_class(Bin<Res>(), 0)
"#,
    );
    assert_refused_at_copy(&source, "        b.put(self.items[0])", "Res");
}

#[test]
fn a_generic_class_method_keeping_places_of_both_parameters_runs_at_plain_types() {
    assert_heap_guard_output(
        &with_bin(
            TRAY,
            r#"
fn main()
    let t = Tray<int>(1)
    let own = Bin<String>()
    t.file_own(List(["a"]), own)
    let mine = Bin<int>()
    t.file_class(mine, "tag")
    println(f"{own.count()} {mine.count()} {t.items[0]}")
"#,
        ),
        "1 1 1",
    );
}

/// A method may not declare a type parameter named after its class's, so a
/// method's own parameter never shadows the class's.
#[test]
fn a_method_type_parameter_named_after_the_class_parameter_is_refused() {
    assert_compiler_error(
        &with_bin(
            r#"
class Shadow<T>
    var items List<T>
    fn init(first T)
        self.items = List([first])
    fn file_first<T>(l List<T>, b Bin<T>)
        b.put(l[0])
"#,
            "\nfn main()\n    println(\"never\")\n",
        ),
        "MER_TYP_064",
    );
}

/// A trait default declaring a type parameter of its own.
const CLERK: &str = r#"
trait Filer
    fn file_first<U>(l List<U>, b Bin<U>)
        b.put(l[0])

class Clerk implements Filer
    fn init()
        return
"#;

#[test]
fn a_trait_default_keeping_a_place_of_its_own_parameter_is_refused_at_a_resource() {
    let source = with_bin(
        CLERK,
        r#"
fn main()
    Clerk().file_first(List([Res(1)]), Bin<Res>())
"#,
    );
    assert_refused_at_copy(&source, "        b.put(l[0])", "Res");
}

#[test]
fn a_trait_default_keeping_a_place_of_its_own_parameter_runs_at_a_plain_type() {
    assert_heap_guard_output(
        &with_bin(
            CLERK,
            r#"
fn main()
    let b = Bin<int>()
    Clerk().file_first(List([7]), b)
    println(f"{b.count()}")
"#,
        ),
        "1",
    );
}

/// An enum method declaring a type parameter of its own.
const MODE: &str = r#"
enum Mode
    Fast
    Slow
    fn mode_first<U>(l List<U>, b Bin<U>)
        b.put(l[0])
"#;

/// A struct holds data only, so no struct method declares a type parameter
/// of its own.
#[test]
fn a_struct_method_declaring_its_own_type_parameter_is_refused() {
    assert_compiler_error(
        &with_bin(
            r#"
struct Clip
    n int
    fn clip_first<U>(l List<U>, b Bin<U>)
        b.put(l[0])
"#,
            "\nfn main()\n    println(\"never\")\n",
        ),
        "MER_TYP_058",
    );
}

#[test]
fn an_enum_method_keeping_a_place_of_its_own_parameter_is_refused_at_a_resource() {
    let source = with_bin(
        MODE,
        r#"
fn main()
    Mode.Fast.mode_first(List([Res(1)]), Bin<Res>())
"#,
    );
    assert_refused_at_copy(&source, "        b.put(l[0])", "Res");
}

#[test]
fn an_enum_method_keeping_a_place_of_its_own_parameter_runs_at_a_plain_type() {
    assert_heap_guard_output(
        &with_bin(
            MODE,
            r#"
fn main()
    let b = Bin<int>()
    Mode.Fast.mode_first(List([8]), b)
    Mode.Slow.mode_first(List([9]), b)
    println(f"{b.count()}")
"#,
        ),
        "2",
    );
}

#[test]
fn a_generic_function_the_program_never_reaches_is_not_refused() {
    assert_heap_guard_output(
        &with_bin(
            FILERS,
            r#"
fn main()
    let b = Bin<Res>()
    println(f"{b.count()}")
"#,
        ),
        "0",
    );
}

/// A generic function is declared at the top level of a module only, so no
/// generic body is nested inside another.
#[test]
fn a_generic_function_declared_inside_a_body_is_refused() {
    assert_compiler_error(
        &with_bin(
            "",
            r#"
fn main()
    fn nested_first<T>(l List<T>, b Bin<T>)
        b.put(l[0])
    nested_first(List([Res(1)]), Bin<Res>())
"#,
        ),
        "MER_TYP_054",
    );
}

/// A module declaring a generic function keeping a place of its parameter.
const FILER_MODULE: &str = r#"use system.collections.list

public class Bin<T>
    var items List<T>
    fn init()
        self.items = List<T>()
    fn put(x T)
        self.items.push(x)
    public fn count() int
        return self.items.length()

public fn file_first<T>(l List<T>, b Bin<T>)
    b.put(l[0])
"#;

/// The project whose `main.mi` imports the filer module, declares a
/// resource, and runs `main_body`.
fn with_filer(main_body: &str) -> [(&'static str, String); 2] {
    let main = format!(
        "use system.io\nuse system.collections.list\nuse local.filer.{{Bin, file_first}}\n\nclass Res\n    public var id int\n    fn init(id int)\n        self.id = id\n    fn drop(self)\n        println(f\"closed {{self.id}}\")\n\nfn main()\n{main_body}"
    );
    [("main.mi", main), ("filer.mi", FILER_MODULE.to_string())]
}

#[test]
fn an_imported_generic_function_keeping_a_place_is_refused_at_a_resource() {
    let files = with_filer("    file_first(List([Res(1)]), Bin<Res>())\n");
    let files: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    let report = check_project_report(&files);
    assert!(report.contains(CODE), "refused with {CODE}:\n{report}");
    assert!(
        report.contains("filer.mi:13"),
        "the refusal points at the module's keep:\n{report}"
    );
    assert!(
        report.contains("compiled at `Res`"),
        "the refusal names the instantiation:\n{report}"
    );
}

#[test]
fn an_imported_generic_function_keeping_a_place_runs_at_a_plain_type() {
    let files = with_filer(
        "    let b = Bin<int>()\n    file_first(List([7]), b)\n    println(f\"{b.count()}\")\n",
    );
    let files: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    assert_project_heap_guard_output(&files, "1");
}
