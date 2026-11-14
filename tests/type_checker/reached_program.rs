// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What the type checker decides a program reaches: each generic body at
//! every set of types the program reaches it at, under the owner its copy is
//! compiled for, with the bodies that led to it.

use crate::type_checker::utils::type_checker_result;
use miri::type_checker::{CopyOwner, ReachedCopy};

/// Each copy the program reaches, spelled `Owner<args>.name<args>`, with a
/// top-level function's owner spelled as the function's module.
fn copies_of(source: &str) -> Vec<String> {
    let result = type_checker_result(source);
    let program = result.type_checker.reached_program();
    let mut copies: Vec<String> = program.copies().iter().map(spelled).collect();
    copies.sort();
    copies
}

fn spelled(copy: &ReachedCopy) -> String {
    let list = |types: &[miri::ast::types::Type]| {
        let kinds: Vec<String> = types.iter().map(|ty| ty.kind.to_string()).collect();
        kinds.join(", ")
    };
    let owner = match &copy.owner {
        CopyOwner::Function(module) => format!("fn {module:?}"),
        CopyOwner::Type { name, arguments } => format!("{name}<{}>", list(arguments)),
    };
    format!("{owner}.{}<{}>", copy.name, list(&copy.arguments))
}

const BOX: &str = "class Box<T>\n    var item T\n    fn init(i T)\n        self.item = i\n    fn unwrap() T\n        return self.item\n\n";

/// A method of a generic class called at two instances is one copy per
/// instance, compiled under the class at each.
#[test]
fn a_method_copy_is_listed_at_each_type_it_is_reached_at() {
    let source = format!(
        "{BOX}fn main()\n    let a = Box<int>(1).unwrap()\n    let b = Box<String>(\"s\").unwrap()\n"
    );
    let copies = copies_of(&source);
    for wanted in ["Box<int>.unwrap<>", "Box<String>.unwrap<>"] {
        assert!(
            copies.iter().any(|copy| copy == wanted),
            "expected {wanted} among {copies:?}"
        );
    }
}

/// A generic function is a copy at the types its call binds, owned by the
/// module declaring it, and the method it calls at its own parameter is
/// reached through it.
#[test]
fn a_copy_reached_through_a_generic_function_names_the_function_it_was_reached_through() {
    let source = format!(
        "{BOX}fn open<T>(x T) T\n    return Box<T>(x).unwrap()\n\nfn main()\n    let a = open(3)\n"
    );
    let result = type_checker_result(&source);
    let program = result.type_checker.reached_program();
    let function = program
        .copies()
        .iter()
        .find(|copy| copy.name == "open")
        .expect("the generic function is reached");
    assert!(matches!(function.owner, CopyOwner::Function(_)));
    assert_eq!(spelled(function), "fn Program.open<int>");
    let method = program
        .copies()
        .iter()
        .find(|copy| spelled(copy) == "Box<int>.unwrap<>")
        .expect("the method the function calls is reached at the function's type");
    let through: Vec<&str> = method.reached_through().map(|(_, name)| name).collect();
    assert_eq!(through, ["open"]);
}

/// The instances ownership is decided over are the reached program's.
#[test]
fn the_reached_program_lists_the_instances_ownership_is_decided_over() {
    let source = format!("{BOX}fn open<T>(x T) T\n    return Box<T>(x).unwrap()\n\nfn main()\n    let a = open(\"s\")\n");
    let result = type_checker_result(&source);
    let checker = &result.type_checker;
    assert_eq!(
        checker.reached_program().instances(),
        checker.ownership_instantiations()
    );
    let boxes = checker.ownership_instantiations().get("Box");
    assert!(
        boxes.is_some_and(|tuples| tuples.iter().any(|args| args
            .first()
            .is_some_and(|arg| arg.kind == miri::ast::types::TypeKind::String))),
        "expected Box<String> among {boxes:?}"
    );
}
