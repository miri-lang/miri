// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which operands the type checker holds: read into a temporary the moment
//! they are evaluated because a later operand of the same sequence may write
//! the place they name. Each program ends `main` with the sequence judged,
//! and each test lists, operand by operand, whether it is held.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::statement::{Statement, StatementKind};
use miri::pipeline::{Pipeline, PipelineResult};

/// The functions and types every program below may use.
const PRELUDE: &str = r#"
use system.collections.list
use system.math

class Box
    var v int
    fn set(x int) int
        self.v = x
        return 0
    fn show(_a int) int: self.v

class Holder
    var b Box
    fn get() Box
        return self.b

struct Point
    x int
    y int

fn bumpi(n out int) int
    n = 99
    return 0

fn other() int: 7

fn id(x int) int: x

fn showi(a int, b int) int: a + b

fn showo(a int, b int?) int: a

fn pick(a List<int>, _b int?) int: a.length()

fn give(a int, b out int) int
    b = a
    return a

fn regrow(x out List<List<int>>) int
    x = List([List([2])])
    return 0
"#;

fn checked(main: &str) -> PipelineResult {
    let source = format!("{PRELUDE}\nfn main()\n{main}");
    match Pipeline::new().frontend(&source) {
        Ok(result) => result,
        Err(error) => panic!("the fixture should type-check: {error:?}"),
    }
}

/// The expression `main`'s last statement evaluates.
fn last_expression(result: &PipelineResult) -> &Expression {
    let main = result
        .ast
        .body
        .iter()
        .find_map(|statement| match &statement.node {
            StatementKind::FunctionDeclaration(declaration) if declaration.name == "main" => {
                declaration.body.as_deref()
            }
            _ => None,
        });
    let Some(Statement {
        node: StatementKind::Block(statements),
        ..
    }) = main
    else {
        panic!("the fixture should declare a block-bodied main");
    };
    match statements.last().map(|statement| &statement.node) {
        Some(StatementKind::Expression(expression)) => expression,
        Some(StatementKind::Variable(declarations, _)) => declarations
            .last()
            .and_then(|declaration| declaration.initializer.as_deref())
            .unwrap_or_else(|| panic!("the last declaration should be initialized")),
        _ => panic!("main should end in an expression or a declaration"),
    }
}

/// The operands `sequence` evaluates, in order.
fn operands(sequence: &Expression) -> Vec<&Expression> {
    match &sequence.node {
        ExpressionKind::Call(callee, arguments) => {
            let receiver = match &callee.node {
                ExpressionKind::Member(object, _) => Some(&**object),
                _ => None,
            };
            receiver.into_iter().chain(arguments).collect()
        }
        ExpressionKind::Binary(left, _, right) | ExpressionKind::Index(left, right) => {
            vec![left, right]
        }
        ExpressionKind::Tuple(elements) => elements.iter().collect(),
        _ => panic!("expected a sequence, found {:?}", sequence.node),
    }
}

/// Whether each operand of `sequence` is held.
fn held(result: &PipelineResult, sequence: &Expression) -> Vec<bool> {
    let operands = operands(sequence);
    let held = &result.type_checker.held_operands;
    operands.into_iter().map(|o| held.holds(o)).collect()
}

/// Whether each operand of the sequence `main` ends in is held.
fn held_in_main(main: &str) -> Vec<bool> {
    let result = checked(main);
    held(&result, last_expression(&result))
}

#[test]
fn an_operand_a_later_out_argument_writes_is_held() {
    let main = "    var n = 1\n    let r = showi(n, bumpi(n))\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn an_operand_a_later_out_argument_of_another_binding_writes_is_not_held() {
    let main = "    var n = 1\n    var m = 2\n    let r = showi(n, bumpi(m))\n";
    assert_eq!(held_in_main(main), [false, false]);
}

#[test]
fn an_operand_an_out_argument_nested_in_a_later_call_writes_is_held() {
    let main = "    var n = 1\n    let r = showi(n, id(bumpi(n)))\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn an_operand_a_later_assignment_writes_is_held() {
    let main = "    var n = 1\n    let r = showi(n, (n = 5))\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn an_operand_whose_collection_a_later_writing_method_changes_is_held() {
    let main = "    var xs = List([1, 2])\n    let r = pick(xs, xs.pop())\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn an_element_whose_index_binding_a_later_operand_writes_is_held() {
    let main = "    let xs = List([1, 2])\n    var i = 0\n    let r = showi(xs[i], bumpi(i))\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn a_class_field_is_held_when_a_later_operand_runs_a_call() {
    let main = "    let c = Box(v: 1)\n    let r = showi(c.v, other())\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn a_struct_field_is_not_held_when_a_later_operand_runs_a_call() {
    let main = "    let p = Point(x: 1, y: 2)\n    let r = showi(p.x, other())\n";
    assert_eq!(held_in_main(main), [false, false]);
}

#[test]
fn a_class_field_is_not_held_when_the_later_operand_calls_an_intrinsic() {
    let main = "    let c = Box(v: 1)\n    let r = showi(c.v, max(1, 2))\n";
    assert_eq!(held_in_main(main), [false, false]);
}

#[test]
fn a_class_field_is_not_held_when_the_later_operand_builds_a_variant() {
    let main = "    let c = Box(v: 1)\n    let r = showo(c.v, Some(1))\n";
    assert_eq!(held_in_main(main), [false, false]);
}

#[test]
fn a_class_field_is_held_when_a_later_operand_assigns_anything() {
    let main = "    let c = Box(v: 1)\n    var k = 0\n    let r = showi(c.v, (k = 3))\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn a_field_of_a_call_result_is_held_when_a_later_operand_runs_a_call() {
    let main = "    let h = Holder(b: Box(v: 1))\n    let r = showi(h.get().v, h.b.set(9))\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn an_out_argument_itself_is_never_held() {
    let main = "    var n = 1\n    let r = give(n, n)\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn a_receiver_no_later_operand_writes_is_not_held() {
    let main = "    let b = Box(v: 1)\n    var n = 1\n    let r = b.show(bumpi(n))\n";
    assert_eq!(held_in_main(main), [false, false]);
}

#[test]
fn an_operators_left_side_the_right_side_writes_is_held() {
    let main = "    var n = 1\n    let r = n + bumpi(n)\n";
    assert_eq!(held_in_main(main), [true, false]);
}

#[test]
fn a_step_of_a_path_a_writing_method_writes_holds_nothing() {
    let main = "    var xss = List([List([1])])\n    xss[regrow(xss)].push(5)\n";
    let result = checked(main);
    let ExpressionKind::Call(callee, _) = &last_expression(&result).node else {
        panic!("main should end in a call");
    };
    let ExpressionKind::Member(path, _) = &callee.node else {
        panic!("the call should be a method call");
    };
    assert_eq!(held(&result, path), [false, false]);
}
