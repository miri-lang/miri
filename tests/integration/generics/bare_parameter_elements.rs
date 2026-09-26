// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Containers whose element type is a bare generic parameter.
//!
//! Inside a generic body the parameter has no concrete type yet, so there is no
//! per-type release helper to register against the container. Registering one
//! anyway names a symbol nothing defines, and the program fails to link rather
//! than to compile — reporting a mangled name instead of the source.

use super::utils::*;

#[test]
fn an_array_literal_of_a_bare_parameter_links() {
    assert_runs_with_output(
        r#"
fn fill<T>(a T) int
    var arr = [a, a]
    return arr.length()

fn main()
    let s = "P".to_lower()
    let n = fill(s)
    println(f"{n}")
"#,
        "2",
    );
}

#[test]
fn a_written_array_type_of_a_bare_parameter_links() {
    // The declared type is not what decides it — the aggregate literal is.
    assert_runs_with_output(
        r#"
fn fill<T>(a T) int
    var arr [T; 2] = [a, a]
    return arr.length()

fn main()
    let s = "P".to_lower()
    let n = fill(s)
    println(f"{n}")
"#,
        "2",
    );
}

#[test]
fn an_array_of_a_bare_parameter_releases_its_managed_elements() {
    // Not registering a release helper inside the generic body must not mean the
    // elements are never released: the instantiation is what knows how, and the
    // program has to come out balanced.
    assert_runs_with_output(
        r#"
fn first_of<T>(a T, b T) T
    var arr = [a, b]
    return arr[0]

fn main()
    let x = "A".to_lower()
    let y = "B".to_lower()
    let got = first_of(x, y)
    println(got)
"#,
        "a",
    );
}

#[test]
fn a_set_of_a_bare_parameter_links() {
    // The set's registration site screens the parameter already; this holds it
    // there, since the guard is what the array literal was missing.
    assert_runs_with_output(
        r#"
use system.collections.set

fn fill<T>(a T) int
    var c = Set<T>()
    c.add(a)
    return c.length()

fn main()
    let s = "P".to_lower()
    let n = fill(s)
    println(f"{n}")
"#,
        "1",
    );
}

#[test]
fn a_list_built_from_a_bare_parameter_links_and_combines() {
    // The same registration site serves a list built from a literal inside a
    // generic body. Combining into the element needs the element's own type,
    // which is why this reads back the sum rather than a truncation of it.
    assert_runs_with_output(
        r#"
use system.collections.list

fn bump<T>(a T, b T) T
    var xs = List([a])
    xs[0] += b
    return xs[0]

fn main()
    let r = bump(1.5, 2.25)
    println(f"{r}")
"#,
        "3.75",
    );
}

#[test]
fn a_list_of_a_bare_parameter_releases_its_managed_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn hold<T>(a T) T
    var xs = List([a])
    return xs[0]

fn main()
    let s = "Q".to_lower()
    let got = hold(s)
    println(got)
"#,
        "q",
    );
}

const LIST_PARAMETER_SEARCHES: &str = r#"
use system.collections.list

class Tag
    public name String
    fn init(name String)
        self.name = name
    public fn equals(other Tag) bool
        return self.name == other.name

fn seek<T>(xs List<T>, item T) bool
    return xs.contains(item)

fn place<T>(xs List<T>, item T) int
    return xs.index_of(item) ?? -1
"#;

/// A generic function handed a `List<T>` reaches the list's search methods at
/// the caller's element type: the strings and the class instances it compares
/// are borrowed, not released, and every one is freed exactly once at exit.
#[test]
fn a_generic_function_searches_a_list_it_is_handed_at_the_callers_element_type() {
    let code = format!(
        "{LIST_PARAMETER_SEARCHES}
fn main()
    let ss = List([\"a\" + \"\", \"b\" + \"\"])
    let probe = \"b\"
    let tags = List([Tag(\"x\" + \"\"), Tag(\"y\" + \"\")])
    let wanted = Tag(\"y\")
    println(f\"{{seek(ss, probe)}} {{place(ss, probe)}} {{seek(tags, wanted)}} {{place(tags, wanted)}}\")
"
    );
    assert_heap_guard_output(&code, "true 1 true 1");
}
