// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method of the declared `Option` called on an optional, whichever way
//! the optional is spelled and laid out: a payload held as bytes, a counted
//! payload held as its own pointer, an optional of an optional, an optional
//! read out of a list, an optional inside a generic function, and the one a
//! map lookup returns. The method's body sees its receiver as the caller
//! holds it.
//!
//! Each program runs under the MIR verifier and the heap guard.

use super::super::utils::*;

#[test]
fn test_is_some_reads_an_optional_held_as_bytes() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let a int? = Some(3)
    let b int? = None
    println(f"{a.is_some()} {a.is_none()} {b.is_some()} {b.is_none()}")
"#,
        "true false false true",
    );
}

#[test]
fn test_is_some_reads_a_counted_payload_held_as_its_pointer() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let a String? = Some("hi")
    let b String? = None
    println(f"{a.is_some()} {b.is_none()}")
"#,
        "true true",
    );
}

#[test]
fn test_is_some_reads_an_optional_class_instance() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var value int

fn main()
    let a Node? = Some(Node(value: 1))
    let b Node? = None
    println(f"{a.is_some()} {b.is_some()}")
"#,
        "true false",
    );
}

#[test]
fn test_is_some_reads_an_optional_of_an_optional_held_as_bytes() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let a Option<int?> = Some(None)
    let b Option<int?> = None
    let c Option<int?> = Some(Some(2))
    println(f"{a.is_some()} {b.is_some()} {c.is_some()}")
"#,
        "true false true",
    );
}

#[test]
fn test_is_some_reads_an_optional_of_a_counted_optional() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let a Option<String?> = Some(None)
    let b Option<String?> = None
    let c Option<String?> = Some(Some("x"))
    println(f"{a.is_some()} {b.is_some()} {c.is_some()}")
"#,
        "true false true",
    );
}

#[test]
fn test_is_some_reads_an_optional_struct_held_inline() {
    assert_heap_guard_output(
        r#"
use system.io

struct V3<T>
    x T
    y T
    z T

fn main()
    let one f32 = 1.0
    let a V3<f32>? = Some(V3(x: one, y: one, z: one))
    let b V3<f32>? = None
    println(f"{a.is_some()} {b.is_none()}")
"#,
        "true true",
    );
}

#[test]
fn test_is_some_reads_an_optional_list_element() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    let xs [int?] = List([Some(1), None])
    let names [String?] = List([Some("n"), None])
    println(f"{xs[0].is_some()} {xs[1].is_some()} {names[0].is_none()} {names[1].is_none()}")
"#,
        "true false false true",
    );
}

#[test]
fn test_is_some_reads_an_optional_inside_a_generic_function() {
    assert_heap_guard_output(
        r#"
use system.io

fn present<T>(o T?) bool
    return o.is_some()

fn main()
    let a int? = Some(1)
    let b String? = None
    println(f"{present(a)} {present(b)}")
"#,
        "true false",
    );
}

#[test]
fn test_is_some_reads_what_a_map_lookup_returns() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn main()
    let m = {"a": 1}
    let names = {"k": "v"}
    println(f"{m.get('a').is_some()} {m.get('b').is_none()} {names.get('k').is_some()}")
"#,
        "true true true",
    );
}

#[test]
fn test_a_member_the_optional_does_not_declare_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let a int? = Some(3)
    println(f"{a.frobnicate()}")
"#,
        "Type 'int?' has no member 'frobnicate'",
    );
}

#[test]
fn test_a_payload_member_asked_of_the_optional_names_how_to_reach_the_payload() {
    assert_compiler_error(
        r#"
use system.io

class Node
    var value int

fn main()
    let a Node? = Some(Node(value: 1))
    println(f"{a.value}")
"#,
        "is read once it is known to be present: `if let Some(v) = x`",
    );
}

/// Copy the directory `from` into `to`, file by file.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn test_a_body_compiled_once_for_every_element_calls_a_method_of_the_optional() {
    // A standard-library collection's methods are compiled once for every
    // element type, so an optional of its element is held there at the
    // collection's own parameter, and the method of the optional it calls is
    // the one compiled at that parameter too.
    let stdlib = tempfile::tempdir().unwrap();
    let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib");
    copy_tree(&source, stdlib.path());
    let list = stdlib.path().join("system/collections/list.mi");
    let text = std::fs::read_to_string(&list).unwrap();
    let anchor = "    public fn pop() T?\n";
    assert!(
        text.contains(anchor),
        "list.mi no longer declares `pop` as expected"
    );
    let patched = text.replacen(
        anchor,
        "    public fn can_pop() bool\n        let p = self.pop()\n        return p.is_some()\n\n    public fn pop() T?\n",
        1,
    );
    std::fs::write(&list, patched).unwrap();

    let program = tempfile::Builder::new().suffix(".mi").tempfile().unwrap();
    std::fs::write(
        program.path(),
        r#"
use system.io
use system.collections.list

class Node
    var v int

fn main()
    var a = List([1, 2])
    var b = List(["x"])
    var c = List<int?>()
    c.push(Some(1))
    var d = List<Node>()
    var e = List<String?>()
    e.push(None)
    println(f"{a.can_pop()} {b.can_pop()} {c.can_pop()} {d.can_pop()} {e.can_pop()}")
"#,
    )
    .unwrap();
    let output = crate::utils::miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib.path())
        .env("MIRI_HEAP_GUARD", "1")
        .arg("run")
        .arg("--verify-mir")
        .arg(program.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.trim() == "true true true false true",
        "got {:?}:\n{stdout}{}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_a_method_of_the_optional_read_as_a_value_is_refused() {
    for declared in ["int?", "Option<int>", "Node?"] {
        let value = if declared == "Node?" {
            "Some(Node(v: 1))"
        } else {
            "Some(1)"
        };
        assert_compiler_error(
            &format!(
                r#"
use system.io

class Node
    var v int

fn main()
    let o {declared} = {value}
    let f = o.is_some
    println(f"{{f()}}")
"#
            ),
            "error[MER_TYP_078]",
        );
    }
}
