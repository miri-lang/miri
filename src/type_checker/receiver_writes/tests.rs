// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which methods write their receiver, read off small classes.

use super::*;
use crate::ast::types::TypeKind;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn parse(source: &str) -> crate::ast::Program {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    match parser.parse() {
        Ok(program) => program,
        Err(error) => panic!("the fixture should parse: {error:?}"),
    }
}

/// The name a class or trait is written with.
fn written_name(expression: &Expression) -> Option<String> {
    if let ExpressionKind::GenericType(inner, _, _) = &expression.node {
        return written_name(inner);
    }
    if let ExpressionKind::Type(ty, _) = &expression.node {
        let TypeKind::Custom(name, _) = &ty.kind else {
            return None;
        };
        return Some(name.clone());
    }
    let ExpressionKind::Identifier(name, _) = &expression.node else {
        return None;
    };
    Some(name.clone())
}

/// The receiver writers `source` declares, as `Class.method`, sorted.
fn writers(source: &str) -> Vec<String> {
    let program = parse(source);
    let writes = ReceiverWrites::of(program.body.iter(), written_name);
    let mut names: Vec<String> = (writes.writers.iter())
        .map(|(class, method)| format!("{class}.{method}"))
        .collect();
    names.sort();
    names
}

/// A class over two runtime entry points of the list runtime: one that only
/// reads its argument and one that writes it.
const BAG: &str = r#"
class Bag
    runtime "core" fn miri_rt_list_len(l Self) int
    runtime "core" fn miri_rt_list_push(l Self, item int)

    fn size() int: miri_rt_list_len(self)

    fn add(item int)
        miri_rt_list_push(self, item)

    fn add_twice(item int)
        self.add(item)
        self.add(item)

    fn add_through_twice(item int)
        self.add_twice(item)

    fn is_small() bool: self.size() < 3
"#;

#[test]
fn a_runtime_call_writes_the_receiver_only_where_the_runtime_does_not_only_read() {
    let found = writers(BAG);
    assert!(found.contains(&"Bag.add".to_string()), "{found:?}");
    assert!(!found.contains(&"Bag.size".to_string()), "{found:?}");
}

#[test]
fn a_call_on_self_of_a_writer_writes_through_every_level() {
    let found = writers(BAG);
    for method in ["Bag.add_twice", "Bag.add_through_twice"] {
        assert!(found.contains(&method.to_string()), "{found:?}");
    }
    assert!(!found.contains(&"Bag.is_small".to_string()), "{found:?}");
}

#[test]
fn an_assignment_into_the_receiver_writes_it() {
    let found = writers(
        r#"
class Grid
    fn clear_first()
        self[0] = 0

    fn first() int: self[0]
"#,
    );
    assert_eq!(found, vec!["Grid.clear_first".to_string()]);
}

#[test]
fn a_call_on_self_of_a_method_with_no_body_anywhere_is_taken_to_write() {
    let found = writers(
        r#"
class Cell
    fn touch()
        self.missing()
"#,
    );
    assert_eq!(found, vec!["Cell.touch".to_string()]);
}

#[test]
fn a_trait_default_runs_as_the_implementing_class_method() {
    let found = writers(
        r#"
trait Filling
    fn fill(item int)
        self.add(item)

    fn peek() int: self.size()

class Bag implements Filling
    runtime "core" fn miri_rt_list_len(l Self) int
    runtime "core" fn miri_rt_list_push(l Self, item int)

    fn size() int: miri_rt_list_len(self)

    fn add(item int)
        miri_rt_list_push(self, item)
"#,
    );
    assert_eq!(found, vec!["Bag.add".to_string(), "Bag.fill".to_string()]);
}

/// A class whose methods hand `self` on in every way but the ones that
/// only read it.
const ALIASES: &str = r#"
fn consume(b Bag)
    b.add(1)

class Bag
    runtime "core" fn miri_rt_list_len(l Self) int
    runtime "core" fn miri_rt_list_push(l Self, item int)
    runtime "core" fn miri_rt_list_reserve(l Self, capacity int)

    fn add(item int)
        miri_rt_list_push(self, item)

    fn reserve_through_a_binding(capacity int)
        var me = self
        miri_rt_list_reserve(me, capacity)

    fn reserve_through_a_cast(capacity int)
        miri_rt_list_reserve(self as Bag, capacity)

    fn hand_over()
        consume(self)

    fn capture() int
        let f = fn() int: miri_rt_list_len(self)
        f()

    fn size() int: miri_rt_list_len(self as Bag)

    fn same(other Bag) bool: self == other

    fn first() int: self[0]

    fn described() String: f"{self}"

    fn itself() Bag: self
"#;

#[test]
fn a_use_of_the_receiver_that_does_not_only_read_it_writes_it() {
    let found = writers(ALIASES);
    for method in [
        "Bag.add",
        "Bag.reserve_through_a_binding",
        "Bag.reserve_through_a_cast",
        "Bag.hand_over",
        "Bag.capture",
    ] {
        assert!(found.contains(&method.to_string()), "{method} in {found:?}");
    }
}

#[test]
fn reading_the_receiver_writes_nothing_of_it() {
    let found = writers(ALIASES);
    for method in [
        "Bag.size",
        "Bag.same",
        "Bag.first",
        "Bag.described",
        "Bag.itself",
    ] {
        assert!(
            !found.contains(&method.to_string()),
            "{method} in {found:?}"
        );
    }
}

#[test]
fn a_method_never_analysed_is_taken_to_write() {
    let writes = ReceiverWrites::default();
    assert!(writes.writes_receiver("List", "pop"));
    assert!(writes.writes_receiver("List", "length"));
    let analysed = ReceiverWrites::of(parse(BAG).body.iter(), written_name);
    assert!(!analysed.writes_receiver("Bag", "size"));
    assert!(analysed.writes_receiver("Bag", "no_such_method"));
    assert!(analysed.writes_receiver("Unseen", "size"));
}

#[test]
fn a_call_on_super_runs_the_parent_body_not_the_override() {
    let found = writers(
        r#"
class Base
    runtime "core" fn miri_rt_list_push(l Self, item int)

    fn add(item int)
        miri_rt_list_push(self, item)

    fn grow()
        self.add(1)

class Child extends Base
    fn grow()
        super.grow()
"#,
    );
    assert!(found.contains(&"Child.grow".to_string()), "{found:?}");
}

/// Every module of the standard library, parsed.
fn standard_library() -> Vec<crate::ast::Program> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/stdlib");
    let mut pending = vec![root];
    let mut programs = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory)
            .unwrap_or_else(|e| panic!("the stdlib directory {directory:?} reads: {e}"));
        let mut paths: Vec<std::path::PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        paths.sort();
        for path in paths {
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "mi") {
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("{path:?} reads: {e}"));
                programs.push(parse(&source));
            }
        }
    }
    programs
}

/// The methods of `class` the standard library's bodies make writers, and
/// those they make readers, each sorted.
fn split_of(writes: &ReceiverWrites, class: &str) -> (Vec<String>, Vec<String>) {
    let mut writers = Vec::new();
    let mut readers = Vec::new();
    for (owner, method) in &writes.analysed {
        if owner != class {
            continue;
        }
        if writes.writers.contains(&(owner.clone(), method.clone())) {
            writers.push(method.clone());
        } else {
            readers.push(method.clone());
        }
    }
    writers.sort();
    readers.sort();
    (writers, readers)
}

#[test]
fn the_standard_library_collections_write_only_through_their_writers() {
    let programs = standard_library();
    let statements = programs.iter().flat_map(|program| program.body.iter());
    let writes = ReceiverWrites::of(statements, written_name);
    for (class, expected) in [
        (
            "List",
            vec![
                "clear",
                "insert",
                "pop",
                "push",
                "remove",
                "remove_at",
                "reserve",
                "resize",
                "reverse",
                "set",
                "sort",
            ],
        ),
        ("Map", vec!["clear", "remove", "set"]),
        ("Set", vec!["add", "clear", "remove"]),
        ("Array", vec!["reverse", "set", "sort"]),
    ] {
        let (writers, readers) = split_of(&writes, class);
        assert_eq!(
            writers, expected,
            "{class} writers; its readers are {readers:?}"
        );
        for reader in ["length", "element_at", "clone"] {
            assert!(
                readers.contains(&reader.to_string()),
                "{class}.{reader} in {readers:?}"
            );
        }
    }
    let (_, list_readers) = split_of(&writes, "List");
    for reader in [
        "get", "contains", "index_of", "first", "last", "is_empty", "equals", "hash",
    ] {
        assert!(
            list_readers.contains(&reader.to_string()),
            "List.{reader} in {list_readers:?}"
        );
    }
}
