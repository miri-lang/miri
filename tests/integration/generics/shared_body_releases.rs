// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A shared generic body — one compiled for every instantiation of its
//! declaration — holds values at instantiations still naming its parameters,
//! and releases them through their type's shared drop function. So do the
//! closures written inside it and the collection entries it builds. A body
//! lowered for one instantiation leaves nothing open; a value inference left
//! partly unbound is released at the arguments it does carry.

use super::utils::*;

const PAIR_AND_WRAPPER: &str = r#"
use system.collections.list

class Pair<T>
    var a T
    fn init(a T)
        self.a = a

class Wrapper<U>
    var x U
    fn init(x U)
        self.x = x
"#;

fn with_wrapper(methods: &str, main: &str) -> String {
    format!("{PAIR_AND_WRAPPER}\n{methods}\n{main}")
}

#[test]
fn a_closure_inside_a_shared_body_releases_what_it_holds() {
    assert_heap_guard_output(
        &with_wrapper(
            r#"
    fn count() int
        let p = Pair<U>(self.x)
        let f = fn() int
            let _q = p
            return 1
        return f()
"#,
            r#"
fn main()
    let w = Wrapper<String>("s" + "t")
    println(f"{w.count()}")
"#,
        ),
        "1",
    );
}

#[test]
fn a_collection_entry_built_in_a_shared_body_is_released() {
    assert_heap_guard_output(
        &with_wrapper(
            r#"
    fn count() int
        var l = List<(Pair<U>, int)>()
        l.push((Pair<U>(self.x), 1))
        return l.length()
"#,
            r#"
fn main()
    let w = Wrapper<String>("s" + "t")
    println(f"{w.count()}")
"#,
        ),
        "1",
    );
}

#[test]
fn an_enum_value_inference_left_partly_unbound_releases_its_payload() {
    assert_heap_guard_output(
        r#"
fn main()
    let r = Result.Ok("s" + "t")
    match r
        Result.Ok(v): println(v)
        Result.Err(_): println("err")
"#,
        "st",
    );
}

#[test]
fn a_list_of_enum_values_inference_left_partly_unbound_releases_each_payload() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let l = [Result.Ok("s" + "t"), Result.Ok("u" + "v")]
    println(f"{l.length()}")
"#,
        "2",
    );
}
