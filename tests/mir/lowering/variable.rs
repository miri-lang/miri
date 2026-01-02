// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::ast::literal::Literal;
use miri::mir::{Operand, Rvalue, StatementKind};

#[test]
fn test_lower_variable_declaration() {
    let source = "fn main(): let x = 10";
    let body = lower_code(source);

    // Check for local 'x'
    let mut found_x = false;
    for decl in &body.local_decls {
        if let Some(name) = &decl.name {
            if name == "x" {
                found_x = true;
                break;
            }
        }
    }
    assert!(found_x, "Did not find local variable 'x'");

    // Check assignment
    // bb0: { _x = const 10; ... }
    let bb0 = &body.basic_blocks[0];
    let mut found_assign = false;
    for stmt in &bb0.statements {
        if let StatementKind::Assign(place, rvalue) = &stmt.kind {
            let local_idx = place.local.0;
            let local_decl = &body.local_decls[local_idx];
            if local_decl.name.as_deref() == Some("x") {
                if let Rvalue::Use(Operand::Constant(c)) = rvalue {
                    if let Literal::Integer(miri::ast::literal::IntegerLiteral::I8(10)) = c.literal
                    {
                        found_assign = true;
                    }
                }
            }
        }
    }
    assert!(found_assign, "Did not find assignment to 'x'");
}

#[test]
fn test_variable_access_and_assignment() {
    let source = "
fn main()
    var x = 1
    var y = x
    x = 2
";
    let body = lower_code(source);

    // Expected locals:
    // _0: return
    // _1: x
    // _2: y
    // _3: temp for assignment result
    assert!(body.local_decls.len() >= 3);

    let bb0 = &body.basic_blocks[0];

    // We expect 3 key assignments:
    // 1. x = 1
    // 2. y = x
    // 3. x = 2

    let mut assign_count = 0;

    for stmt in &bb0.statements {
        if let StatementKind::Assign(place, rvalue) = &stmt.kind {
            // Check if we are assigning to x or y
            let local_idx = place.local.0;
            if local_idx < body.local_decls.len() {
                let local_decl = &body.local_decls[local_idx];
                if let Some(name) = &local_decl.name {
                    if name == "x" {
                        // Check values
                        match rvalue {
                            Rvalue::Use(Operand::Constant(c)) => {
                                // Should be 1 or 2
                                if let Literal::Integer(miri::ast::literal::IntegerLiteral::I8(1)) =
                                    c.literal
                                {
                                    assign_count += 1;
                                } else if let Literal::Integer(
                                    miri::ast::literal::IntegerLiteral::I8(2),
                                ) = c.literal
                                {
                                    assign_count += 1;
                                }
                            }
                            _ => {}
                        }
                    } else if name == "y" {
                        // Should be assignment from x
                        if let Rvalue::Use(Operand::Copy(p)) = rvalue {
                            // Check if p is x
                            let src_local = &body.local_decls[p.local.0];
                            if src_local.name.as_deref() == Some("x") {
                                assign_count += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    assert_eq!(assign_count, 3, "Expected 3 key assignments: x=1, y=x, x=2");
}
