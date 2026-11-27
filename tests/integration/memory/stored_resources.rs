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

#[test]
fn a_resource_written_into_an_array_literal_moves_there() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let l = [t, Token(id: 2)]
    println(f"held {l.length()}")
"#,
        ),
        "held 2\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_resource_read_after_an_array_literal_moved_it_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let l = [t]
    println(f"{t.id} {l.length()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_resource_written_into_an_array_literal_twice_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let l = [t, t]
    println(f"{l.length()}")
"#,
        ),
        "is written into the array literal more than once",
    );
}

#[test]
fn a_payload_written_into_an_array_literal_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r)
            let l = [r]
            println(f"held {l.length()}")
        None: println("none")
    println("after")
"#,
        ),
        "held 1\nreleased 1\nafter",
    );
}

#[test]
fn a_payload_of_a_nested_optional_written_into_an_array_literal_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let p Option<Token?> = Some(Some(Token(id: 0)))
    match p
        Some(q)
            match q
                Some(r)
                    let l = [Token(id: 5), r]
                    println(f"last {l[1].id}")
                None: println("inner none")
        None: println("none")
    println("after")
"#,
        ),
        "last 0\nreleased 5\nreleased 0\nafter",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_written_into_an_array_literal_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 0))
    match p
        Some(r)
            let l = [r]
            println(f"{l.length()}")
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_payload_stored_by_an_index_assignment_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List([Token(id: 9)])
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): l[0] = r
        None: println("none")
    println(f"first {l[0].id}")
"#,
        ),
        "released 9\nfirst 1\nreleased 1",
    );
}

#[test]
fn a_payload_stored_by_an_index_assignment_on_some_paths_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List([Token(id: 0)])
    var source = List([Token(id: 1), Token(id: 2)])
    while source.length() > 0
        match source.remove_at(0)
            Some(r):
                if r.id == 2: l[0] = r
            None: break
    println(f"first {l[0].id}")
"#,
        ),
        "released 1\nreleased 0\nfirst 2\nreleased 2",
    );
}

#[test]
fn a_payload_stored_by_a_map_index_assignment_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
use system.collections.map

fn main()
    var m = Map<int, Token>()
    let p Token? = Some(Token(id: 4))
    match p
        Some(r): m[7] = r
        None: println("none")
    println(f"size {m.length()}")
"#,
        ),
        "size 1\nreleased 4",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_stored_by_an_index_assignment_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    var l = List([Token(id: 9)])
    let p Token? = Some(Token(id: 0))
    match p
        Some(r): l[0] = r
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_stored_by_a_map_index_assignment_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
use system.collections.map

fn main()
    var m = Map<int, Token>()
    let p Token? = Some(Token(id: 0))
    match p
        Some(r): m[1] = r
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_payload_pushed_onto_a_list_of_optionals_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List<Token?>()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): l.push(r)
        None: println("none")
    println(f"size {l.length()}")
"#,
        ),
        "size 1\nreleased 1",
    );
}

#[test]
fn a_payload_pushed_onto_a_list_of_optionals_on_some_paths_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var source = List([Token(id: 1), Token(id: 2), Token(id: 3)])
    var kept = List<Token?>()
    while source.length() > 0
        match source.remove_at(0)
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
fn a_payload_inserted_into_a_list_of_optionals_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List<Token?>()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): l.insert(0, r)
        None: println("none")
    println(f"size {l.length()}")
"#,
        ),
        "size 1\nreleased 1",
    );
}

#[test]
fn a_payload_stored_as_an_optional_map_value_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
use system.collections.map

fn main()
    var m = Map<int, Token?>()
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
fn a_payload_stored_by_an_index_assignment_into_an_optional_element_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var l = List<Token?>()
    l.push(Token(id: 9))
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): l[0] = r
        None: println("none")
    println(f"size {l.length()}")
"#,
        ),
        "released 9\nsize 1\nreleased 1",
    );
}

#[test]
fn a_payload_stored_by_a_map_index_assignment_into_an_optional_value_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
use system.collections.map

fn main()
    var m = Map<int, Token?>()
    let p Token? = Some(Token(id: 4))
    match p
        Some(r): m[7] = r
        None: println("none")
    println(f"size {m.length()}")
"#,
        ),
        "size 1\nreleased 4",
    );
}

#[test]
fn a_payload_of_a_nested_optional_pushed_onto_a_list_of_optionals_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var kept = List<Token?>()
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
fn an_optional_read_after_its_payload_was_pushed_onto_a_list_of_optionals_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    var kept = List<Token?>()
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

#[test]
fn a_payload_binding_matched_again_and_read_by_a_quiet_method_in_the_inner_arm_has_one_holder() {
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
                None: println(f"inner none {q.is_none()}")
        None: println("none")
    println("after")
"#,
        ),
        "shelved\nreleased 0\nafter",
    );
}

#[test]
fn an_empty_payload_binding_matched_again_and_read_by_a_quiet_method_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn shelve(t Token) Slot
    return Slot(held: Some(t))

fn main()
    let p Option<Token?> = Some(None)
    match p
        Some(q)
            match q
                Some(r)
                    let _s = shelve(r)
                    println("shelved")
                None: println(f"inner none {q.is_none()}")
        None: println("none")
    println("after")
"#,
        ),
        "inner none true\nafter",
    );
}

/// A resource that a set can hold: a hashed key whose release runs a hook.
const KEY: &str = r#"
class Key
    public var id int
    fn equals(self, other Key) bool: return self.id == other.id
    fn hash(self) int: return self.id
    fn drop(self)
        println(f"released key {self.id}")
"#;

fn with_token_and_key(program: &str) -> String {
    format!("{TOKEN}{KEY}{program}")
}

#[test]
fn a_resource_written_into_a_tuple_literal_moves_there() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let l = (t, 7)
    println(f"held {l.0.id} {l.1}")
"#,
        ),
        "held 1 7\nreleased 1",
    );
}

#[test]
fn a_resource_read_after_a_tuple_literal_moved_it_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let l = (t, 1)
    println(f"{l.1}")
    println(f"{t.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_resource_written_into_a_tuple_literal_twice_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let l = (t, t)
    println(f"{l.0.id}")
"#,
        ),
        "is written into the tuple literal more than once",
    );
}

#[test]
fn a_payload_written_into_a_tuple_literal_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r)
            let l = (Token(id: 5), r)
            println(f"held {l.1.id}")
        None: println("none")
    println("after")
"#,
        ),
        "held 1\nreleased 5\nreleased 1\nafter",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_written_into_a_tuple_literal_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 0))
    match p
        Some(r)
            let l = (r, 1)
            println(f"{l.1}")
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

/// [`assert_one_holder_output`] for a program printing `expected` in any
/// order: a set or a map releases its elements in an unspecified one.
fn assert_one_holder_lines(code: &str, expected: &[&str]) {
    let result = miri_run_with_env(code, "MIRI_HOOK_CHECK", "1");
    assert!(
        result.success,
        "expected a clean run under MIRI_HOOK_CHECK=1:\n{}",
        result.output()
    );
    let mut lines: Vec<&str> = result.stdout.lines().collect();
    lines.sort_unstable();
    let mut expected = expected.to_vec();
    expected.sort_unstable();
    assert_eq!(lines, expected, "{}", result.stderr);
}

#[test]
fn a_resource_written_into_a_set_literal_moves_there() {
    assert_one_holder_lines(
        &with_token_and_key(
            r#"
fn main()
    let k = Key(id: 1)
    let s = {k, Key(id: 2)}
    println(f"held {s.length()}")
"#,
        ),
        &["held 2", "released key 1", "released key 2"],
    );
}

#[test]
fn an_entry_read_beside_a_resource_a_map_literal_moves_sees_it_whole() {
    assert_one_holder_lines(
        &with_token_and_key(
            r#"
use system.collections.map

fn main()
    let t = Token(id: 1)
    let m = {1: t, t.id + 1: Token(id: t.id + 1)}
    println(f"held {m.length()} {m[2].id}")
"#,
        ),
        &["held 2 2", "released 1", "released 2"],
    );
}

#[test]
fn a_resource_read_after_a_set_literal_moved_it_is_refused() {
    assert_compiler_error(
        &with_token_and_key(
            r#"
fn main()
    let k = Key(id: 1)
    let s = {k}
    println(f"{s.length()}")
    println(f"{k.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_resource_written_into_a_set_literal_twice_is_refused() {
    assert_compiler_error(
        &with_token_and_key(
            r#"
fn main()
    let k = Key(id: 1)
    let s = {k, k}
    println(f"{s.length()}")
"#,
        ),
        "is written into the set literal more than once",
    );
}

#[test]
fn a_payload_written_into_a_set_literal_has_one_holder() {
    assert_one_holder_output(
        &with_token_and_key(
            r#"
fn main()
    let p Key? = Some(Key(id: 1))
    match p
        Some(r)
            let s = {r}
            println(f"held {s.length()}")
        None: println("none")
    println("after")
"#,
        ),
        "held 1\nreleased key 1\nafter",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_written_into_a_set_literal_is_refused() {
    assert_compiler_error(
        &with_token_and_key(
            r#"
fn main()
    let p Key? = Some(Key(id: 0))
    match p
        Some(r)
            let s = {r}
            println(f"{s.length()}")
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_resource_written_into_a_map_literal_moves_there() {
    assert_one_holder_output(
        &with_token_and_key(
            r#"
fn main()
    let t = Token(id: 1)
    let k = Key(id: 2)
    let m = {1: t}
    let n = {k: 3}
    println(f"held {m.length()} {n.length()}")
"#,
        ),
        "held 1 1\nreleased key 2\nreleased 1",
    );
}

#[test]
fn a_resource_read_after_a_map_literal_moved_it_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let m = {1: t}
    println(f"{m.length()}")
    println(f"{t.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_resource_written_into_a_map_literal_twice_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let t = Token(id: 1)
    let m = {1: t, 2: t}
    println(f"{m.length()}")
"#,
        ),
        "is written into the map literal more than once",
    );
}

#[test]
fn a_payload_written_into_a_map_literal_has_one_holder() {
    assert_one_holder_output(
        &with_token_and_key(
            r#"
fn main()
    let p Token? = Some(Token(id: 1))
    let q Key? = Some(Key(id: 2))
    match p
        Some(r)
            let m = {4: r}
            println(f"value {m.length()}")
        None: println("none")
    match q
        Some(k)
            let n = {k: 4}
            println(f"key {n.length()}")
        None: println("none")
    println("after")
"#,
        ),
        "value 1\nreleased 1\nkey 1\nreleased key 2\nafter",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_written_into_a_map_literal_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 0))
    match p
        Some(r)
            let m = {1: r}
            println(f"{m.length()}")
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_payload_written_into_an_optional_tuple_element_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r)
            let l (Token?, int) = (r, 2)
            println(f"held {l.1}")
        None: println("none")
    println("after")
"#,
        ),
        "held 2\nreleased 1\nafter",
    );
}

#[test]
fn a_payload_written_into_an_optional_array_element_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r)
            let l [Token?; 2] = [r, None]
            println(f"held {l.length()}")
        None: println("none")
    println("after")
"#,
        ),
        "held 2\nreleased 1\nafter",
    );
}

/// `TOKEN` with a class holding a `Token` field of its own, not optional.
fn with_token_and_box(program: &str) -> String {
    format!("{TOKEN}\nclass Crate\n    public var held Token\n{program}")
}

#[test]
fn a_payload_bound_by_let_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r)
            let b = r
            println(f"bound {b.id}")
        None: println("none")
    println("after")
"#,
        ),
        "bound 1\nreleased 1\nafter",
    );
}

#[test]
fn a_payload_bound_by_let_on_some_paths_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var source = List([Token(id: 1), Token(id: 2)])
    while source.length() > 0
        match source.remove_at(0)
            Some(r):
                if r.id == 2
                    let b = r
                    println(f"bound {b.id}")
            None: break
    println("after")
"#,
        ),
        "released 1\nbound 2\nreleased 2\nafter",
    );
}

#[test]
fn a_payload_stored_into_a_variable_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var x = Token(id: 9)
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): x = r
        None: println("none")
    println(f"held {x.id}")
"#,
        ),
        "released 9\nheld 1\nreleased 1",
    );
}

#[test]
fn a_payload_stored_into_an_optional_variable_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var x Token? = None
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): x = r
        None: println("none")
    println(f"held {x.is_some()}")
"#,
        ),
        "held true\nreleased 1",
    );
}

#[test]
fn a_payload_stored_into_a_field_has_one_holder() {
    assert_one_holder_output(
        &with_token_and_box(
            r#"
fn main()
    var c = Crate(held: Token(id: 9))
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): c.held = r
        None: println("none")
    println(f"held {c.held.id}")
"#,
        ),
        "released 9\nheld 1\nreleased 1",
    );
}

#[test]
fn a_payload_stored_into_an_optional_field_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var s = Slot(held: None)
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): s.held = r
        None: println("none")
    println(f"held {s.held.is_some()}")
"#,
        ),
        "held true\nreleased 1",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_bound_by_let_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    let p Token? = Some(Token(id: 0))
    match p
        Some(r)
            let b = r
            println(f"{b.id}")
        None: println("none")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_stored_into_a_variable_is_refused() {
    assert_compiler_error(
        &with_token(
            r#"
fn main()
    var x = Token(id: 9)
    let p Token? = Some(Token(id: 0))
    match p
        Some(r): x = r
        None: println("none")
    println(f"{p.is_some()} {x.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn an_optional_read_after_its_payload_was_stored_into_a_field_is_refused() {
    assert_compiler_error(
        &with_token_and_box(
            r#"
fn main()
    var c = Crate(held: Token(id: 9))
    let p Token? = Some(Token(id: 0))
    match p
        Some(r): c.held = r
        None: println("none")
    println(f"{p.is_some()} {c.held.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

#[test]
fn a_payload_stored_into_an_optional_variable_on_some_paths_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var x Token? = None
    var source = List([Token(id: 1), Token(id: 2), Token(id: 3)])
    while source.length() > 0
        match source.remove_at(0)
            Some(r):
                if r.id != 2: x = r
            None: break
    println(f"held {x.is_some()}")
"#,
        ),
        "released 2\nreleased 1\nheld true\nreleased 3",
    );
}

#[test]
fn a_payload_stored_back_into_the_optional_it_was_matched_out_of_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var p Token? = Some(Token(id: 1))
    match p
        Some(r): p = r
        None: println("none")
    println(f"held {p.is_some()}")
"#,
        ),
        "held true\nreleased 1",
    );
}

/// `TOKEN` with a class holding a `Token` two optional layers deep.
fn with_token_and_deep_slot(program: &str) -> String {
    format!("{TOKEN}\nclass Deep\n    public var held Option<Token?>\n{program}")
}

#[test]
fn a_resource_pushed_two_optional_layers_deep_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var kept = List<Option<Token?>>()
    kept.push(Token(id: 1))
    println(f"kept {kept.length()}")
"#,
        ),
        "kept 1\nreleased 1",
    );
}

#[test]
fn a_payload_pushed_two_optional_layers_deep_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn main()
    var kept = List<Option<Token?>>()
    let p Token? = Some(Token(id: 1))
    match p
        Some(r): kept.push(r)
        None: println("none")
    println(f"kept {kept.length()}")
"#,
        ),
        "kept 1\nreleased 1",
    );
}

#[test]
fn a_resource_passed_two_optional_layers_deep_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn show(o Option<Token?>)
    match o
        Some(inner)
            match inner
                Some(t): println(f"shown {t.id}")
                None: println("inner none")
        None: println("none")

fn main()
    show(Token(id: 1))
    println("after")
"#,
        ),
        "shown 1\nreleased 1\nafter",
    );
}

#[test]
fn a_resource_stored_in_a_two_layer_field_has_one_holder() {
    assert_one_holder_output(
        &with_token_and_deep_slot(
            r#"
fn main()
    let d = Deep(held: Token(id: 1))
    println(f"held {d.held.is_some()}")
    var e = Deep(held: None)
    e.held = Token(id: 2)
    println(f"held {e.held.is_some()}")
"#,
        ),
        "held true\nheld true\nreleased 2\nreleased 1",
    );
}

#[test]
fn a_resource_returned_or_bound_two_optional_layers_deep_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn wrap(t Token) Option<Token?>
    return t

fn main()
    let w = wrap(Token(id: 1))
    println(f"wrapped {w.is_some()}")
    let b Option<Token?> = Token(id: 2)
    println(f"bound {b.is_some()}")
"#,
        ),
        "wrapped true\nbound true\nreleased 2\nreleased 1",
    );
}

#[test]
fn a_resource_wrapped_three_optional_layers_deep_before_an_early_return_has_one_holder() {
    assert_one_holder_output(
        &with_token(
            r#"
fn early(n int) Option<Option<Token?>>
    var kept = List<Option<Option<Token?>>>()
    for i in 0..n
        kept.push(Token(id: 10 + i))
        if i == 1: return Token(id: 99)
    return None

fn main()
    let e = early(3)
    println(f"early {e.is_some()}")
"#,
        ),
        "released 10\nreleased 11\nearly true\nreleased 99",
    );
}
