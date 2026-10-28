// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A call lends its arguments: the callee reads the caller's reference and
// takes none of its own. An argument read out of a class field is held only
// by that field, and a callee that reaches the instance another way can
// replace or empty the field while it still reads the argument. These pin
// that such an argument stays alive and unchanged for the whole call, at
// every optimization level, and that one lent to a callee that can release
// nothing is passed without a retain.

use crate::utils::miri_cmd;
use std::path::PathBuf;

/// `probe(h, h.node)`: the callee replaces `h.node`, releasing the node it
/// still reads as `n`, then allocates a node that may reuse its block.
const REPLACED_FIELD: &str = r#"
use system.io

class Node
    var v int

    fn init(v int)
        self.v = v

class Holder
    var node Node

    fn init(node Node)
        self.node = node

    fn reset()
        self.node = Node(99)

fn probe(h Holder, n Node) int
    h.reset()
    let junk = Node(55)
    n.v + junk.v

let h = Holder(Node(3))
println(f"{probe(h, h.node)}")
"#;

/// `probe(h, h.items)`: the callee empties `h.items` in place while it still
/// reads the list as `xs`.
const EMPTIED_FIELD: &str = r#"
use system.io
use system.collections.list

class Node
    var v int

    fn init(v int)
        self.v = v

class Holder
    var items [Node]

    fn init(items [Node])
        self.items = items

    fn wipe()
        self.items.clear()

fn probe(h Holder, xs [Node]) int
    let n0 = xs.length()
    h.wipe()
    n0 * 10 + xs.length()

let h = Holder(List([Node(3), Node(4)]))
println(f"{probe(h, h.items)}")
"#;

/// `peek(h, h.node)`: the callee only reads, so nothing it runs can release
/// the node it is lent. (Building an instance would not be read-only: its
/// `init` stores into the fields of its receiver.)
const READ_ONLY_CALLEE: &str = r#"
use system.io

class Node
    var v int

    fn init(v int)
        self.v = v

class Holder
    var node Node

    fn init(node Node)
        self.node = node

fn peek(h Holder, n Node) int
    n.v + h.node.v * 10

let h = Holder(Node(3))
println(f"{peek(h, h.node)}")
"#;

/// The directory the standard library is read from.
fn stdlib_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib")
}

/// Run `code` at optimization `level` under the heap guard, the leak check,
/// MIR verification and a scribbling allocator, and assert it prints
/// `expected` with every check silent.
fn assert_output_at(code: &str, level: &str, expected: &str) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("prog.mi");
    std::fs::write(&source, code).unwrap();
    let output = miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib_path())
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .env("MallocScribble", "1")
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .args(["run", "--opt-level", level])
        .arg(&source)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && !stderr.contains("MIRI_HEAP_GUARD:"),
        "expected a clean run at opt level {level}:\n{stdout}{stderr}"
    );
    assert_eq!(
        stdout.trim(),
        expected,
        "output at opt level {level}:\n{stdout}{stderr}"
    );
}

/// The body of `function` as `miri build --emit mir` writes it for `code`.
fn emitted_body(code: &str, function: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("prog.mi");
    std::fs::write(&source, code).unwrap();
    let out = dir.path().join("prog");
    miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib_path())
        .arg("build")
        .arg(&source)
        .arg("--out")
        .arg(&out)
        .args(["--emit", "mir"])
        .assert()
        .success();
    let mir = std::fs::read_to_string(dir.path().join("prog.mir")).unwrap();
    let header = format!("fn {function}\n");
    let start = mir
        .find(&header)
        .unwrap_or_else(|| panic!("no `{function}` in:\n{mir}"));
    let body = &mir[start + header.len()..];
    body[..body.find("\nfn ").unwrap_or(body.len())].to_string()
}

/// The argument at `position` of the one call `body` makes to `callee`.
fn argument_of(body: &str, callee: &str, position: usize) -> String {
    let call = format!("fn {callee}(");
    let at = body
        .find(&call)
        .unwrap_or_else(|| panic!("no call of `{callee}` in:\n{body}"));
    let rest = &body[at + call.len()..];
    let list = &rest[..rest.find(')').unwrap_or(rest.len())];
    list.split(", ")
        .nth(position)
        .unwrap_or_default()
        .to_string()
}

#[test]
fn an_argument_read_from_a_field_the_callee_replaces_stays_alive() {
    for level in ["0", "3"] {
        assert_output_at(REPLACED_FIELD, level, "58");
    }
}

#[test]
fn a_list_read_from_a_field_the_callee_empties_keeps_its_elements() {
    for level in ["0", "3"] {
        assert_output_at(EMPTIED_FIELD, level, "22");
    }
}

#[test]
fn a_field_argument_lent_to_a_read_only_callee_runs_correctly() {
    for level in ["0", "3"] {
        assert_output_at(READ_ONLY_CALLEE, level, "33");
    }
}

#[test]
fn a_field_argument_to_a_callee_that_replaces_the_field_is_retained_for_the_call() {
    let main = emitted_body(REPLACED_FIELD, "main");
    let argument = argument_of(&main, "miri.probe", 1);
    assert_ne!(argument, "_2.0", "the field is lent unretained:\n{main}");
    assert!(
        main.contains("IncRef(_2.0)"),
        "no retain of the field:\n{main}"
    );
}

#[test]
fn a_field_argument_to_a_callee_that_only_reads_is_lent_without_a_retain() {
    let main = emitted_body(READ_ONLY_CALLEE, "main");
    assert_eq!(argument_of(&main, "miri.peek", 1), "_2.0", "{main}");
    assert!(
        !main.contains("IncRef(_2.0)"),
        "a retain of the field for a callee that releases nothing:\n{main}"
    );
}

/// `h.items.map(f)`: the list is the receiver of a method that only reads
/// it, while `f` replaces `h.items`, releasing the list `map` walks.
const MAPPED_FIELD: &str = r#"
use system.io
use system.collections.list

class Node
    var v int
    fn init(v int)
        self.v = v

class Holder
    var items [Node]
    fn init(items [Node])
        self.items = items
    fn repl()
        self.items = List([Node(5)])

fn go(h Holder) int
    let f = fn(n Node) Node
        h.repl()
        let junk = List([Node(1000), Node(2000), Node(3000)])
        Node(n.v + junk.length())
    let r = h.items.map(f)
    r.length() * 100 + r[r.length() - 1].v

let h = Holder(List([Node(3), Node(4), Node(6)]))
println(f"{go(h)}")
"#;

/// A holder whose `repl` replaces both a list field and a map field, with a
/// closure-taking method on each called through the field.
const REPLACING_CLOSURES: &str = r#"
use system.io
use system.collections.list
use system.collections.map

class Holder
    var items [int]
    var m {int: int}
    fn init()
        self.items = List([1, 2, 3, 4, 5, 6, 7, 8])
        self.m = {1: 10, 2: 20, 3: 30}
    fn repl()
        self.items = List([5])
        self.m = {9: 9}

fn a(h Holder) int
    let f = fn(x int) bool
        h.repl()
        let junk = List([100, 200, 300, 400, 500, 600, 700, 800])
        x > 1000
    let r = h.items.any(f)
    if r
        return 1
    0

fn b(h Holder) int
    let f = fn(acc int, x int) int
        h.repl()
        let junk = List([100, 200, 300, 400, 500, 600, 700, 800])
        acc + x
    h.items.reduce(0, f)

fn c(h Holder) int
    let f = fn(k int, v int) bool
        h.repl()
        let junk = {7: 7, 8: 8, 9: 9}
        true
    h.m.filter(f).length()
"#;

/// `p1(h, h.m[1])`: the node is read out of a map a field holds, and the
/// callee clears that map.
const MAP_ENTRY_OF_A_FIELD: &str = r#"
use system.io
use system.collections.map

class Node
    var v int
    fn init(v int)
        self.v = v

class Holder
    var m {int: Node}
    fn init()
        self.m = {1: Node(5)}
    fn clr()
        self.m.clear()

fn p1(h Holder, n Node) int
    h.clr()
    let junk = Node(55)
    n.v + junk.v

fn main()
    let h2 = Holder()
    println(f"{p1(h2, h2.m[1])}")
"#;

/// `p1(h, n)` with `n` bound out of `h.m.get(1)`: the callee clears the map
/// the node was read from.
const MAP_GET_OF_A_FIELD: &str = r#"
use system.io
use system.collections.map

class Node
    var v int
    fn init(v int)
        self.v = v

class Holder
    var m {int: Node}
    fn init()
        self.m = {1: Node(5)}
    fn clr()
        self.m.clear()

fn p1(h Holder, n Node) int
    h.clr()
    let junk = Node(55)
    n.v + junk.v

fn main()
    let h2 = Holder()
    match h2.m.get(1)
        Some(n): println(f"{p1(h2, n)}")
        None: println("none")
"#;

/// `REPLACING_CLOSURES` run through `call`.
fn replacing_closures_calling(call: &str) -> String {
    format!("{REPLACING_CLOSURES}\nfn main()\n    println(f\"{{{call}(Holder())}}\")\n")
}

#[test]
fn a_field_list_mapped_while_the_closure_replaces_it_stays_alive() {
    for level in ["0", "3"] {
        assert_output_at(MAPPED_FIELD, level, "309");
    }
}

#[test]
fn a_field_list_searched_while_the_closure_replaces_it_stays_alive() {
    let code = replacing_closures_calling("a");
    for level in ["0", "3"] {
        assert_output_at(&code, level, "0");
    }
}

#[test]
fn a_field_list_reduced_while_the_closure_replaces_it_stays_alive() {
    let code = replacing_closures_calling("b");
    for level in ["0", "3"] {
        assert_output_at(&code, level, "36");
    }
}

#[test]
fn a_field_map_filtered_while_the_closure_replaces_it_stays_alive() {
    let code = replacing_closures_calling("c");
    for level in ["0", "3"] {
        assert_output_at(&code, level, "3");
    }
}

#[test]
fn a_map_entry_of_a_field_the_callee_clears_stays_alive() {
    for level in ["0", "3"] {
        assert_output_at(MAP_ENTRY_OF_A_FIELD, level, "60");
    }
}

#[test]
fn a_map_value_got_from_a_field_the_callee_clears_stays_alive() {
    for level in ["0", "3"] {
        assert_output_at(MAP_GET_OF_A_FIELD, level, "60");
    }
}

#[test]
fn a_field_receiver_of_a_method_that_only_reads_it_is_retained_for_the_call() {
    let go = emitted_body(MAPPED_FIELD, "miri.go");
    assert_ne!(
        argument_of(&go, "miri.List$Node.map", 0),
        "_1.0",
        "the receiver is lent unretained:\n{go}"
    );
}

#[test]
fn a_field_receiver_the_callee_writes_in_place_is_lent_as_made_unique() {
    let wipe = emitted_body(EMPTIED_FIELD, "miri.Holder.wipe");
    assert_eq!(argument_of(&wipe, "miri.List.clear", 0), "_1.0", "{wipe}");
}

/// A holder whose set and map hold `Elem`s, a class whose own `equals` and
/// `hash` replace both collections, so matching an element runs code that
/// releases the collection being searched.
const MATCHING_RUNS_PROGRAM_CODE: &str = r#"
use system.io
use system.collections.list
use system.collections.set
use system.collections.map

class Elem
    var h Holder
    var k int
    fn init(h Holder, k int)
        self.h = h
        self.k = k
    fn equals(other Elem) bool
        self.h.repl()
        let junk = List([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20])
        self.k == other.k
    fn hash() int
        self.h.repl()
        let junk = List([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20])
        self.k
    fn drop()
        self.h.repl()
        let junk = List([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20])

class Holder
    var items [Elem]
    var s {Elem}
    var m {Elem: int}
    var armed bool
    fn init()
        self.items = List<Elem>()
        self.s = Set<Elem>()
        self.m = Map<Elem, int>()
        self.armed = false
    fn arm()
        self.armed = true
    fn repl()
        if self.armed
            self.armed = false
            self.items = List<Elem>()
            self.s = Set<Elem>()
            self.m = Map<Elem, int>()
"#;

/// `MATCHING_RUNS_PROGRAM_CODE`, then a `main` that arms the holder and
/// prints `search`, a lookup in one of its collections.
fn searching(search: &str) -> String {
    format!(
        "{MATCHING_RUNS_PROGRAM_CODE}\nfn main()\n    let h = Holder()\n    h.items.push(Elem(h, 1))\n    \
         h.s.add(Elem(h, 3))\n    h.m.set(Elem(h, 3), 3)\n    h.arm()\n    let ok = {search}\n    \
         println(f\"{{ok}}\")\n"
    )
}

/// A set of ints a field holds, matched by the runtime alone.
const INT_SET_FIELD: &str = r#"
use system.io
use system.collections.set

class Holder
    var s {int}
    fn init()
        self.s = Set<int>()

fn main()
    let h = Holder()
    h.s.add(3)
    println(f"{h.s.contains(3)}")
"#;

#[test]
fn a_field_set_searched_with_an_equals_that_replaces_it_stays_alive() {
    let code = searching("h.s.contains(Elem(h, 3))");
    for level in ["0", "3"] {
        assert_output_at(&code, level, "true");
    }
}

#[test]
fn a_field_map_searched_with_a_hash_that_replaces_it_stays_alive() {
    let code = searching("h.m.contains_key(Elem(h, 3))");
    for level in ["0", "3"] {
        assert_output_at(&code, level, "true");
    }
}

#[test]
fn a_field_set_whose_matching_runs_program_code_is_retained_for_the_search() {
    let main = emitted_body(&searching("h.s.contains(Elem(h, 3))"), "main");
    assert_ne!(
        argument_of(&main, "miri.Set.contains", 0),
        "_2.1",
        "the set is lent unretained:\n{main}"
    );
}

#[test]
fn a_field_set_of_ints_is_searched_without_a_retain() {
    let main = emitted_body(INT_SET_FIELD, "main");
    assert_eq!(argument_of(&main, "miri.Set.contains", 0), "_2.0", "{main}");
}
