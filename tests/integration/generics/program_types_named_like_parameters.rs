// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A program type named like a library type parameter stays the program's
//! type inside the library's bodies, and the parameter stays what its
//! instance binds it to.
//!
//! `Pair<String, K>`, with `K` a program struct, binds the library's `K` to
//! `String` and its `V` to the program's `K`. A type the body reads, the
//! tuple, the array, the optional or the enum value it builds, the list it
//! writes, is read at the instance once, where it is read: reading it again
//! cannot tell the library's `K` from the program's, and reading it never
//! leaves the library's `K` naming the program's struct.

use super::utils::*;

const PAIRS: &str = r#"
use system.collections.list

public enum Slot<T>
    Full(T)
    Empty

public class Pair<K, V>
    public var key K
    public var value V
    fn init(key K, value V)
        self.key = key
        self.value = value

    public fn from_tuple() K
        let t = (self.key, self.key)
        return t.1

    public fn from_array() K
        let ks = [self.key, self.key]
        return ks[1]

    public fn from_optional() K
        return match Some(self.key)
            Some(k): k
            None: self.key

    public fn from_slot() K
        let s = Slot.Full(self.key)
        return match s
            Slot.Full(k): k
            Slot.Empty: self.key

    public fn from_list() K
        var ks = List<K>()
        ks.push(self.key)
        ks[0] = self.key + "!"
        return ks[0]
"#;

/// The entry file reading `read` off a `Pair<String, K>` whose `K` is the
/// program's own struct.
fn main_reading(read: &str) -> String {
    format!(
        "use local.pairs.{{Pair}}\n\
         \n\
         struct K\n    a int\n    b float\n\
         \n\
         fn main()\n    \
         let p = Pair<String, K>(\"k\" + \"1\", K(a: 1, b: 2.5))\n    \
         println(f\"{{p.{read}()}} {{p.value.b}}\")\n"
    )
}

fn assert_reads(read: &str, expected: &str) {
    let main = main_reading(read);
    assert_project_runs_with_output(&[("main.mi", &main), ("pairs.mi", PAIRS)], expected);
}

#[test]
fn a_tuple_of_the_key_parameter_holds_the_instance_key() {
    assert_reads("from_tuple", "k1 2.5");
}

#[test]
fn an_array_of_the_key_parameter_holds_the_instance_key() {
    assert_reads("from_array", "k1 2.5");
}

#[test]
fn an_optional_of_the_key_parameter_holds_the_instance_key() {
    assert_reads("from_optional", "k1 2.5");
}

#[test]
fn an_enum_value_over_the_key_parameter_holds_the_instance_key() {
    assert_reads("from_slot", "k1 2.5");
}

#[test]
fn a_list_of_the_key_parameter_written_by_index_holds_the_instance_key() {
    assert_reads("from_list", "k1! 2.5");
}
