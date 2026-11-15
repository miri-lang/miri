// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function or method declared in an imported module keeps a resource the
//! way one declared in the program does: the caller's reference transfers at
//! the call, the hook runs where the callee's new owner ends, and a later use
//! of the argument is refused. Where a callee is declared does not change
//! when a hook runs. Every test reads the exact order of the hooks' output
//! under the heap guard.

use super::super::utils::*;

const KEEPERS: &str = r#"use system.io

public class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

struct Shelf
    slot Res?

public fn keep(r Res) int
    if true
        let _shelf = Shelf(slot: Some(r))
        println("kept")
    println("leaving keep")
    return 1

public fn pass_on(r Res) int
    return keep(r)

public fn keep_own() int
    let r = Res(5)
    return keep(r)

public class Keeper
    fn take(r Res) int
        if true
            let _shelf = Shelf(slot: Some(r))
            println("taken")
        println("leaving take")
        return 2

struct DerivedShelf
    slot Derived?

public class Derived
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed derived {self.id}")
    fn shelve(n int) int
        if true
            let _shelf = DerivedShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n
"#;

/// The project whose `main.mi` imports the keepers and runs `main_body`.
fn with_keepers(main_body: &str) -> [(&'static str, String); 2] {
    let main = format!(
        "use system.io\nuse local.keepers.{{Res, keep, pass_on, keep_own, Keeper, Derived}}\n\nfn main()\n{main_body}"
    );
    [("main.mi", main), ("keepers.mi", KEEPERS.to_string())]
}

fn assert_keepers_output(main_body: &str, expected: &str) {
    let files = with_keepers(main_body);
    let files: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    assert_project_heap_guard_output(&files, expected);
}

fn assert_keepers_refused(main_body: &str) {
    let files = with_keepers(main_body);
    let files: Vec<(&str, &str)> = files.iter().map(|(n, s)| (*n, s.as_str())).collect();
    assert_project_compiler_error(&files, "MER_OWN_003");
}

/// A module function keeping its parameter owns it: the hook runs where the
/// shelf holding it ends, inside the call.
#[test]
fn a_module_function_keeping_a_resource_releases_it_where_its_keeper_ends() {
    assert_keepers_output(
        "    let h = Res(1)\n    println(f\"{keep(h)}\")\n    println(\"end\")\n",
        "kept\nclosed 1\nleaving keep\n1\nend",
    );
}

#[test]
fn a_resource_a_module_function_kept_cannot_be_used_after() {
    assert_keepers_refused("    let h = Res(1)\n    let _n = keep(h)\n    println(f\"{h.id}\")\n");
}

/// A module function handing its owned parameter on to another keeper hands
/// its reference over by move.
#[test]
fn a_module_function_handing_its_parameter_on_moves_it_to_the_keeper() {
    assert_keepers_output(
        "    let h = Res(1)\n    println(f\"{pass_on(h)}\")\n    println(\"end\")\n",
        "kept\nclosed 1\nleaving keep\n1\nend",
    );
}

/// A module function handing a local of its own to a keeper hands it over
/// by move, and ends without releasing it again.
#[test]
fn a_module_function_handing_its_own_local_on_moves_it_to_the_keeper() {
    assert_keepers_output(
        "    println(f\"{keep_own()}\")\n    println(\"end\")\n",
        "kept\nclosed 5\nleaving keep\n1\nend",
    );
}

/// A module method keeping its receiver owns it.
#[test]
fn a_module_method_keeping_its_receiver_releases_it_where_its_keeper_ends() {
    assert_keepers_output(
        "    let d = Derived(1)\n    println(f\"{d.shelve(1)}\")\n    println(\"end\")\n",
        "shelved\nclosed derived 1\nleaving shelve\n1\nend",
    );
}

#[test]
fn a_receiver_a_module_method_kept_cannot_be_used_after() {
    assert_keepers_refused(
        "    let d = Derived(1)\n    println(f\"{d.shelve(1)}\")\n    println(f\"{d.shelve(2)}\")\n",
    );
}

/// A module method keeping a parameter owns it.
#[test]
fn a_module_method_keeping_a_parameter_releases_it_where_its_keeper_ends() {
    assert_keepers_output(
        "    let k = Keeper()\n    let h = Res(2)\n    println(f\"{k.take(h)}\")\n    println(\"end\")\n",
        "taken\nclosed 2\nleaving take\n2\nend",
    );
}

#[test]
fn a_parameter_a_module_method_kept_cannot_be_used_after() {
    assert_keepers_refused(
        "    let k = Keeper()\n    let h = Res(2)\n    let _n = k.take(h)\n    println(f\"{h.id}\")\n",
    );
}

/// A module body using a resource after handing it to a keeper is refused,
/// and the refusal names the module's file.
#[test]
fn a_module_body_using_a_resource_after_a_keeper_took_it_is_refused_in_its_file() {
    let library =
        format!("{KEEPERS}\npublic fn reuse(r Res) int\n    let _n = keep(r)\n    return r.id\n");
    let main = "use system.io\nuse local.keepers.{Res, reuse}\n\nfn main()\n    println(f\"{reuse(Res(1))}\")\n";
    let files = [("main.mi", main), ("keepers.mi", library.as_str())];
    assert_project_compiler_error(&files, "MER_OWN_003");
    assert_project_compiler_error(&files, "keepers.mi:");
}

/// A module's `equals`, which a container calls on elements it only lends,
/// is refused for keeping its parameter, in the module's file.
#[test]
fn a_module_equals_keeping_its_parameter_is_refused_in_its_file() {
    let library = concat!(
        "use system.io\n",
        "\n",
        "struct Shelf\n",
        "    slot Tag?\n",
        "\n",
        "public class Tag\n",
        "    public var id int\n",
        "    fn init(id int)\n",
        "        self.id = id\n",
        "    fn drop(self)\n",
        "        println(f\"dropped {self.id}\")\n",
        "    public fn equals(other Tag) bool\n",
        "        let _kept = Shelf(slot: Some(other))\n",
        "        return true\n",
    );
    let main = "use system.io\nuse local.tags.{Tag}\n\nfn main()\n    let a = Tag(1)\n    let b = Tag(2)\n    println(f\"{a == b}\")\n";
    let files = [("main.mi", main), ("tags.mi", library)];
    assert_project_compiler_error(&files, "MER_OWN_006");
    assert_project_compiler_error(&files, "tags.mi:");
}

/// A module body handing a resource to a keeper on one branch cannot use it
/// after the branches join.
#[test]
fn a_module_body_using_a_resource_a_branch_handed_to_a_keeper_is_refused() {
    let library = format!(
        "{KEEPERS}\npublic fn maybe(r Res, c bool) int\n    if c\n        let _n = keep(r)\n    return r.id\n"
    );
    let main = "use system.io\nuse local.keepers.{Res, maybe}\n\nfn main()\n    println(f\"{maybe(Res(1), true)}\")\n";
    let files = [("main.mi", main), ("keepers.mi", library.as_str())];
    assert_project_compiler_error(&files, "MER_OWN_003");
    assert_project_compiler_error(&files, "keepers.mi:");
}

/// A module body handing one resource to a keeper on every turn of a loop
/// is refused at the second turn.
#[test]
fn a_module_body_handing_a_resource_to_a_keeper_in_a_loop_is_refused() {
    let library = format!(
        "{KEEPERS}\npublic fn each(r Res) int\n    var i = 0\n    while i < 2\n        let _n = keep(r)\n        i += 1\n    return i\n"
    );
    let main = "use system.io\nuse local.keepers.{Res, each}\n\nfn main()\n    println(f\"{each(Res(1))}\")\n";
    let files = [("main.mi", main), ("keepers.mi", library.as_str())];
    assert_project_compiler_error(&files, "MER_OWN_003");
    assert_project_compiler_error(&files, "keepers.mi:");
}

const PRIVATE_KEEPERS: &str = r#"use system.io

private class Helper
    var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed helper {self.id}")
    fn shelve(n int) int
        if true
            let _shelf = HelperShelf(slot: Some(self))
            println("shelved")
        println("leaving shelve")
        return n

private struct HelperShelf
    slot Helper?

private class Token
    var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed token {self.id}")

private struct Shelf
    slot Token?

private fn keep(t Token) int
    if true
        let _shelf = Shelf(slot: Some(t))
        println("kept")
    println("leaving keep")
    return 1

public fn run() int
    let h = Helper(1)
    let a = h.shelve(1)
    let t = Token(2)
    return a + keep(t)
"#;

/// A type a module keeps private owns what its methods keep, and a module
/// function keeping a value of one owns it, as for a public type.
#[test]
fn a_module_private_resource_is_released_where_its_keeper_ends() {
    let main =
        "use system.io\nuse local.private_keepers.{run}\n\nfn main()\n    println(f\"{run()}\")\n";
    assert_project_heap_guard_output(
        &[("main.mi", main), ("private_keepers.mi", PRIVATE_KEEPERS)],
        "shelved\nclosed helper 1\nleaving shelve\nkept\nclosed token 2\nleaving keep\n2",
    );
}

const SHADOWING_LIBRARY: &str = r#"use system.io

public class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

private class Keeper
    fn push(r Res) int
        return 7

public fn helper() int
    return 3
"#;

/// The program's keeper, beside a module that keeps a type of the same name
/// private, in `main_body`.
fn with_shadowed_keeper(main_body: &str) -> String {
    format!(
        "use system.io\nuse local.shadowing.{{Res, helper}}\n\nstruct Shelf\n    slot Res?\n\nclass Keeper\n    fn push(r Res) int\n        if true\n            let _shelf = Shelf(slot: Some(r))\n            println(\"pushed\")\n        println(\"leaving push\")\n        return 1\n\nfn main()\n{main_body}"
    )
}

/// A class a module keeps private is no other file's: a method it declares
/// does not share a key with the program's class of the same name, whose
/// keeper still owns what it keeps.
#[test]
fn a_module_private_class_sharing_a_program_class_name_leaves_its_keeper_owning() {
    let main = with_shadowed_keeper(
        "    let k = Keeper()\n    let h = Res(1)\n    println(f\"{k.push(h)} {helper()}\")\n    println(\"end\")\n",
    );
    assert_project_heap_guard_output(
        &[("main.mi", &main), ("shadowing.mi", SHADOWING_LIBRARY)],
        "pushed\nclosed 1\nleaving push\n1 3\nend",
    );
}

#[test]
fn a_resource_the_programs_keeper_took_beside_a_private_namesake_cannot_be_used_after() {
    let main = with_shadowed_keeper(
        "    let k = Keeper()\n    let h = Res(1)\n    let _n = k.push(h)\n    println(f\"{h.id} {helper()}\")\n",
    );
    assert_project_compiler_error(
        &[("main.mi", &main), ("shadowing.mi", SHADOWING_LIBRARY)],
        "MER_OWN_003",
    );
}

const PROGRAM_STACK: &str = r#"
class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

struct Shelf
    slot Res?

class Stack
    fn push(r Res) int
        if true
            let _shelf = Shelf(slot: Some(r))
            println("pushed")
        println("leaving push")
        return 1
"#;

/// A program class named like a library class keeps by its own body.
#[test]
fn a_program_class_named_like_a_library_class_keeps_by_its_own_body() {
    assert_heap_guard_output(
        &format!("{PROGRAM_STACK}\nfn main()\n    let s = Stack()\n    let h = Res(1)\n    println(f\"{{s.push(h)}}\")\n    println(\"end\")\n"),
        "pushed\nclosed 1\nleaving push\n1\nend",
    );
    assert_compiler_error(
        &format!("{PROGRAM_STACK}\nfn main()\n    let s = Stack()\n    let h = Res(1)\n    let _n = s.push(h)\n    println(f\"{{h.id}}\")\n"),
        "MER_OWN_003",
    );
}

const MODULE_HOLDER: &str = r#"use system.collections.list

public class Holder<T>
    var items List<T>
    fn init()
        self.items = List<T>()
    fn add_twice(item T)
        self.items.push(item)
        self.items.push(item)
    fn count() int
        return self.items.length()
"#;

/// A user module's generic class reusing a value it handed to a keeper is
/// refused at a resource instantiation, as the same class in the program's
/// own file is, in the module's file.
#[test]
fn a_user_module_generic_body_reusing_a_kept_value_is_refused_at_a_resource() {
    let main = concat!(
        "use system.io\n",
        "use local.holder.{Holder}\n",
        "\n",
        "class Res\n",
        "    public var id int\n",
        "    fn init(id int)\n",
        "        self.id = id\n",
        "    fn drop(self)\n",
        "        println(f\"closed {self.id}\")\n",
        "\n",
        "fn main()\n",
        "    var h = Holder<Res>()\n",
        "    h.add_twice(Res(1))\n",
        "    println(f\"{h.count()}\")\n",
    );
    let files = [("main.mi", main), ("holder.mi", MODULE_HOLDER)];
    assert_project_compiler_error(&files, "MER_OWN_003");
    assert_project_compiler_error(&files, "holder.mi:");
}

/// The library's generic bodies that reuse a value after handing it on —
/// `resize` pushing one value again and again, `filter` storing what its
/// predicate was handed — hand a funded copy at a resource, so every hook
/// runs once, where the last holder ends.
#[test]
fn library_bodies_reusing_a_resource_run_each_hook_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    public fn equals(other Res) bool
        return self.id == other.id
    public fn hash() int
        return self.id

fn grow() int
    var l = List<Res>()
    l.resize(3, Res(1))
    println(f"resized {l.length()}")
    l.resize(1, Res(2))
    println(f"shrunk {l.length()}")
    return l.length()

fn pick() int
    var m = Map<int, Res>()
    m.set(1, Res(10))
    m.set(2, Res(20))
    let kept = m.filter(fn(k int, v Res) bool: v.id > 10)
    println(f"map kept {kept.length()}")
    return kept.length()

fn choose() int
    var s = Set<Res>()
    s.add(Res(30))
    s.add(Res(40))
    let kept = s.filter(fn(r Res) bool: r.id > 30)
    println(f"set kept {kept.length()}")
    return kept.length()

fn main()
    println(f"{grow()}")
    println(f"{pick()}")
    println(f"{choose()}")
    println("end")
"#,
        "resized 3\nclosed 2\nshrunk 1\nclosed 1\n1\nmap kept 1\nclosed 20\nclosed 10\n1\nset kept 1\nclosed 30\nclosed 40\n1\nend",
    );
}
