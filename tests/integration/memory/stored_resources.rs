// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A resource has one owner, so storing it — in a variable, a field, an
//! element or a collection — moves it there: its hook runs where the new
//! holder lets it go, a later use of the old binding is refused, and no
//! block whose release runs a hook ever has a second holder, which
//! `MIRI_HOOK_CHECK=1` traps.

use super::super::utils::*;
use crate::utils::miri_run_with_env;

const TOKEN: &str = r#"
use system.collections.list

class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

class Slot
    public var held Token?
"#;

fn with_token(program: &str) -> String {
    format!("{TOKEN}{program}")
}

/// Run `code` with the hook check on, expecting `expected` and a clean exit.
fn assert_one_holder_output(code: &str, expected: &str) {
    let result = miri_run_with_env(code, "MIRI_HOOK_CHECK", "1");
    assert!(
        result.success,
        "expected a clean run under MIRI_HOOK_CHECK=1:\n{}",
        result.output()
    );
    assert_eq!(result.stdout.trim_end(), expected, "{}", result.stderr);
}

#[test]
fn reassigning_a_variable_moves_the_resource_into_it() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    var u = Token(id: 2)
    u = t
    println("after move")
    u = Token(id: 3)
    println("after replace")
"#,
        ),
        "released 2\nafter move\nreleased 1\nafter replace\nreleased 3",
    );
}

#[test]
fn a_resource_read_after_a_reassignment_moved_it_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    var u = Token(id: 2)
    u = t
    println(f"{t.id}")
"#,
        ),
        "Use of Moved Value",
    );
}

#[test]
fn a_variable_assigned_to_itself_keeps_its_resource() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var u = Token(id: 1)
    u = u
    println(f"kept {u.id}")
"#,
        ),
        "kept 1\nreleased 1",
    );
}

#[test]
fn a_resource_reassigned_on_one_branch_is_released_once_on_each_path() {
    assert_one_holder_output(
        &with_token(
            r#"
fn pick(take bool)
    let t = Token(id: 1)
    var u = Token(id: 2)
    if take
        u = t
    println("picked")

fn main()
    pick(true)
    pick(false)
"#,
        ),
        "released 2\npicked\nreleased 1\npicked\nreleased 2\nreleased 1",
    );
}

#[test]
fn storing_a_resource_in_a_field_moves_it_there() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var s = Slot(held: None)
    let w = Token(id: 4)
    s.held = w
    println("stored")
    s.held = None
    println("cleared")
"#,
        ),
        "stored\nreleased 4\ncleared",
    );
}

#[test]
fn a_resource_read_after_a_field_store_moved_it_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    var s = Slot(held: None)
    let w = Token(id: 4)
    s.held = w
    println(f"{w.id}")
"#,
        ),
        "Use of Moved Value",
    );
}

#[test]
fn storing_a_resource_in_an_element_moves_it_there() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List<Token>()
    l.push(Token(id: 1))
    let t = Token(id: 2)
    l[0] = t
    println("stored")
    l.clear()
    println("cleared")
"#,
        ),
        "released 1\nstored\nreleased 2\ncleared",
    );
}

#[test]
fn pushing_a_resource_hands_the_list_its_only_reference() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    var l = List<Token>()
    l.push(t)
    println("pushed")
    l.clear()
    println("cleared")
"#,
        ),
        "pushed\nreleased 1\ncleared",
    );
}

#[test]
fn reading_a_resource_element_or_field_borrows_it() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List<Token>()
    l.push(Token(id: 1))
    println(f"{l[0].id}")
    for t in l
        println(f"each {t.id}")
    let s = Slot(held: Token(id: 2))
    if let Some(h) = s.held
        println(f"held {h.id}")
"#,
        ),
        "1\neach 1\nheld 2\nreleased 2\nreleased 1",
    );
}

#[test]
fn a_resource_cast_to_its_trait_as_it_is_stored_moves() {
    assert_one_holder_output(
        r#"
use system.collections.list

trait Named
    fn name(self) int

class Tag implements Named
    public var id int
    fn name(self) int
        return self.id
    fn drop(self)
        println(f"tag {self.id}")

fn main()
    var l = List<Named>()
    let t = Tag(id: 1)
    l.push(t)
    println(f"{l[0].name()}")
    l.clear()
    println("cleared")
"#,
        "1\ntag 1\ncleared",
    );
}

#[test]
fn a_constructor_storing_a_resource_takes_it_over() {
    assert_one_holder_output(
        &with_token(
            r#"
class Holder
    var t Token
    fn init(t Token)
        self.t = t

fn main()
    let t = Token(id: 1)
    if true
        let _h = Holder(t)
        println("built")
    println("after")
"#,
        ),
        "built\nreleased 1\nafter",
    );
}

#[test]
fn a_constructor_only_reading_a_resource_leaves_its_release_to_the_call() {
    assert_one_holder_output(
        &with_token(
            r#"
class Reader
    public var id int
    fn init(t Token)
        self.id = t.id

fn main()
    let t = Token(id: 1)
    let r = Reader(t)
    println(f"read {r.id}")
"#,
        ),
        "released 1\nread 1",
    );
}

#[test]
fn an_inherited_constructor_storing_a_resource_takes_it_over() {
    assert_one_holder_output(
        &with_token(
            r#"
class Base
    var t Token
    fn init(t Token)
        self.t = t

class Derived extends Base

fn main()
    let d = Derived(Token(id: 1))
    println("built")
"#,
        ),
        "built\nreleased 1",
    );
}

#[test]
fn a_generic_constructor_storing_a_resource_takes_it_over() {
    assert_one_holder_output(
        &with_token(
            r#"
class Cell<T>
    var value T
    fn init(value T)
        self.value = value

fn main()
    let t = Token(id: 1)
    let c = Cell<Token>(t)
    println("built")
"#,
        ),
        "built\nreleased 1",
    );
}

#[test]
fn a_constructor_storing_an_optional_resource_takes_it_over() {
    assert_one_holder_output(
        &with_token(
            r#"
class Shelf
    var held Token?
    fn init(held Token?)
        self.held = held

fn main()
    let t = Token(id: 1)
    if true
        let _s = Shelf(Some(t))
        println("built")
    println("after")
"#,
        ),
        "built\nreleased 1\nafter",
    );
}

#[test]
fn a_function_keeping_an_optional_resource_takes_it_over() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token?) Slot
    return Slot(held: t)

fn main()
    let u Token? = Some(Token(id: 2))
    let s = shelve(u)
    println("kept")
"#,
        ),
        "kept\nreleased 2",
    );
}

#[test]
fn an_optional_resource_handed_to_a_keeper_cannot_be_used_again() {
    assert_compiler_error(
        &with_token(
            r#"
fn shelve(t Token?) Slot
    return Slot(held: t)

fn main()
    let u Token? = Some(Token(id: 2))
    let s = shelve(u)
    println(f"{u.is_some()}")
"#,
        ),
        "Use of Moved Value",
    );
}

#[test]
fn removing_a_resource_from_a_list_hands_the_caller_its_only_reference() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List<Token>()
    l.push(Token(id: 1))
    l.push(Token(id: 2))
    l.push(Token(id: 3))
    if let Some(t) = l.remove_at(0)
        println(f"removed {t.id}")
    if let Some(t) = l.pop()
        println(f"popped {t.id}")
    println(f"left {l.length()}")
"#,
        ),
        "removed 1\nreleased 1\npopped 3\nreleased 3\nleft 1\nreleased 2",
    );
}

#[test]
fn the_payload_of_a_matched_result_handed_to_a_keeper_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    var l = List<Token>()
    l.push(Token(id: 1))
    match l.remove_at(0)
        Some(t)
            let _s = shelve(t)
            println("shelved")
        None: println("none")
    println("after")
"#,
        ),
        "shelved\nreleased 1\nafter",
    );
}

#[test]
fn matching_an_optional_resource_held_in_a_binding_gives_it_no_second_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let o Token? = Some(Token(id: 2))
    match o
        Some(t): println(f"matched {t.id}")
        None: println("none")
    if let Some(t) = o
        println(f"again {t.id}")
    println("after")
"#,
        ),
        "matched 2\nagain 2\nafter\nreleased 2",
    );
}

#[test]
fn a_payload_handed_to_a_keeper_out_of_a_matched_binding_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    let o Token? = Some(Token(id: 1))
    match o
        Some(t)
            let _s = shelve(t)
            println("shelved")
        None: println("none")
    var p Token? = None
    match p
        Some(t)
            let _s = shelve(t)
        None: p = Some(Token(id: 2))
    println("after")
"#,
        ),
        "shelved\nreleased 1\nafter\nreleased 2",
    );
}

#[test]
fn a_payload_handed_on_after_another_argument_runs_code_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
class Shelf
    var held Token
    fn init(held Token)
        self.held = held

class Tag
    public var n int
    fn init(n int)
        self.n = n
        println("tag made")

fn shelve(tag Tag, t Token) Shelf
    println(f"tag {tag.n}")
    return Shelf(t)

fn main()
    var l = List<Token>()
    l.push(Token(id: 1))
    match l.remove_at(0)
        Some(t)
            let _s = shelve(Tag(7), t)
            println("shelved")
        None: println("none")
    println("after")
"#,
        ),
        "tag made\ntag 7\nshelved\nreleased 1\nafter",
    );
}

#[test]
fn a_resource_picked_out_of_a_tuple_at_run_time_is_borrowed() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let t = (Token(id: 1), Token(id: 2))
    var i = 1
    let r = t[i]
    println(f"got {r.id}")
"#,
        ),
        "got 2\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_payload_handed_to_a_keeper_on_some_paths_of_its_arm_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    var i = 0
    while i < 2
        let p Token? = Some(Token(id: i))
        match p
            Some(r)
                if r.id == 0
                    let _s = shelve(r)
                    println("shelved")
                else
                    println(f"kept {r.id}")
            None: println("none")
        i += 1
    println("after")
"#,
        ),
        "shelved\nreleased 0\nkept 1\nreleased 1\nafter",
    );
}

#[test]
fn a_payload_handed_to_a_keeper_under_a_condition_is_released_once_where_it_is_not() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    var i = 0
    while i < 2
        let p Token? = Some(Token(id: i))
        match p
            Some(r)
                if r.id == 0
                    let _s = shelve(r)
            None: println("none")
        println(f"end of round {i}")
        i += 1
    println("after")
"#,
        ),
        "released 0\nend of round 0\nend of round 1\nreleased 1\nafter",
    );
}

#[test]
fn a_payload_handed_over_after_the_arm_reads_the_optional_through_a_method_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    var i = 0
    while i < 2
        let p Token? = Some(Token(id: i + 3))
        match p
            Some(r)
                println(f"has {p.is_some()}")
                if r.id == 3
                    let _s = shelve(r)
            None: println("none")
        i += 1
    println("after")
"#,
        ),
        "has true\nreleased 3\nhas true\nreleased 4\nafter",
    );
}

#[test]
fn a_matched_resource_enum_lent_to_a_function_inside_its_arm_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
enum Pack
    Two(Token, Token)
    Empty
    fn drop(self)
        println("pack dropped")
    fn size(self) int
        match self
            Pack.Two(_, _): return 2
            Pack.Empty: return 0

fn helper(p Pack) int
    return p.size()

fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, b)
            println(f"size {helper(p)}")
            println(f"{a.id} {b.id}")
        Pack.Empty: println("none")
"#,
        ),
        "size 2\n1 2\npack dropped\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_payload_of_a_nested_optional_handed_to_a_keeper_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    let p Option<Token?> = Some(Some(Token(id: 0)))
    match p
        Some(q)
            match q
                Some(r)
                    let _s = shelve(r)
                    println("shelved")
                None: println("inner none")
        None: println("none")
    println("after")
"#,
        ),
        "shelved\nreleased 0\nafter",
    );
}

#[test]
fn a_payload_of_a_nested_optional_handed_to_a_keeper_on_some_paths_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    var i = 0
    while i < 2
        let p Option<Token?> = Some(Some(Token(id: i)))
        match p
            Some(q)
                match q
                    Some(r)
                        if r.id == 0
                            let _s = shelve(r)
                            println("shelved")
                        else
                            println(f"kept {r.id}")
                    None: println("inner none")
            None: println("none")
        i += 1
    println("after")
"#,
        ),
        "shelved\nreleased 0\nkept 1\nreleased 1\nafter",
    );
}

#[test]
fn the_payload_of_a_boxed_optional_layer_handed_to_a_keeper_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token?) Slot
    return Slot(held: t)

fn main()
    let p Option<Token?> = Some(Some(Token(id: 0)))
    match p
        Some(q)
            let _s = shelve(q)
            println("shelved")
        None: println("none")
    println("after")
"#,
        ),
        "shelved\nreleased 0\nafter",
    );
}

#[test]
fn an_outer_payload_read_after_its_inner_payload_was_handed_to_a_keeper_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    let p Option<Token?> = Some(Some(Token(id: 0)))
    match p
        Some(q)
            match q
                Some(r)
                    let _s = shelve(r)
                None: println("inner none")
            println(f"{q.is_some()}")
        None: println("none")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_payload_pushed_onto_a_list_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List([Token(id: 1), Token(id: 2)])
    var kept = List<Token>()
    match l.remove_at(0)
        Some(r): kept.push(r)
        None: println("none")
    println(f"kept {kept.length()}")
"#,
        ),
        "kept 1\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_payload_pushed_onto_a_list_on_some_paths_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List([Token(id: 1), Token(id: 2), Token(id: 3)])
    var kept = List<Token>()
    while l.length() > 0
        match l.remove_at(0)
            Some(r):
                if r.id != 2: kept.push(r)
            None: break
    println(f"kept {kept.length()}")
"#,
        ),
        "released 2\nkept 2\nreleased 1\nreleased 3",
    );
}

#[test]
fn a_payload_inserted_into_a_list_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var kept = List([Token(id: 1)])
    let p Token? = Some(Token(id: 2))
    match p
        Some(r): kept.insert(0, r)
        None: println("none")
    println(f"first {kept[0].id}")
"#,
        ),
        "first 2\nreleased 2\nreleased 1",
    );
}

#[test]
fn a_payload_stored_as_a_map_value_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
use system.collections.map

fn main()
    var m = Map<int, Token>()
    let p Token? = Some(Token(id: 4))
    match p
        Some(r): m.set(7, r)
        None: println("none")
    println(f"size {m.length()}")
"#,
        ),
        "size 1\nreleased 4",
    );
}

#[test]
fn a_payload_of_a_nested_optional_pushed_onto_a_list_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var kept = List<Token>()
    let p Option<Token?> = Some(Some(Token(id: 0)))
    match p
        Some(q)
            match q
                Some(r): kept.push(r)
                None: println("inner none")
        None: println("none")
    println(f"kept {kept.length()}")
"#,
        ),
        "kept 1\nreleased 0",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_pushed_onto_a_list_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    var kept = List<Token>()
    let p Token? = Some(Token(id: 0))
    match p
        Some(r): kept.push(r)
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}
