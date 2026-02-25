// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::OptimizationPass;
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::mir::{Body, Constant, Local, Operand, Rvalue, StatementKind, TerminatorKind};
use std::collections::HashMap;

/// Replaces uses of locals with their known constant values and folds
/// binary/unary operations on constants at compile time.
///
/// This pass collects all `_x = const` assignments, then substitutes uses of
/// `_x` with the constant. Binary and unary operations on constants are folded
/// into a single constant. `SwitchInt` terminators with constant discriminants
/// are simplified to `Goto`.
pub struct ConstantPropagation;

impl OptimizationPass for ConstantPropagation {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;
        let mut known_consts: HashMap<Local, Constant> = HashMap::new();

        // 1. Collect constants
        for block in &body.basic_blocks {
            for stmt in &block.statements {
                if let StatementKind::Assign(place, Rvalue::Use(Operand::Constant(c))) = &stmt.kind
                {
                    if place.projection.is_empty() {
                        known_consts.insert(place.local, *c.clone());
                    }
                }
            }
        }

        // 2. Propagate and Fold
        for block in &mut body.basic_blocks {
            for stmt in &mut block.statements {
                if let StatementKind::Assign(_, rvalue) = &mut stmt.kind {
                    match rvalue {
                        Rvalue::Use(op) => {
                            if let Operand::Copy(place) | Operand::Move(place) = op {
                                if place.projection.is_empty() {
                                    if let Some(c) = known_consts.get(&place.local) {
                                        *op = Operand::Constant(Box::new(c.clone()));
                                        changed = true;
                                    }
                                }
                            }
                        }
                        Rvalue::BinaryOp(bin_op, lhs, rhs) => {
                            // Try to resolve operands
                            resolve_operand(lhs, &known_consts);
                            resolve_operand(rhs, &known_consts);

                            // Fold if both are constants
                            // lhs and rhs are &mut Operand
                            if let (Operand::Constant(l), Operand::Constant(r)) = (&**lhs, &**rhs) {
                                if let Some(res) = fold_binary(*bin_op, l, r) {
                                    *rvalue = Rvalue::Use(Operand::Constant(Box::new(res)));
                                    changed = true;
                                }
                            }
                        }
                        Rvalue::UnaryOp(un_op, operand) => {
                            resolve_operand(operand, &known_consts);
                            if let Operand::Constant(c) = &**operand {
                                if let Some(res) = fold_unary(*un_op, c) {
                                    *rvalue = Rvalue::Use(Operand::Constant(Box::new(res)));
                                    changed = true;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // 3. Fold Terminators
            if let Some(term) = &mut block.terminator {
                if let TerminatorKind::SwitchInt {
                    discr,
                    targets,
                    otherwise,
                } = &mut term.kind
                {
                    resolve_operand(discr, &known_consts);
                    if let Operand::Constant(c) = discr {
                        if let Literal::Integer(val) = &c.literal {
                            let int_val = match val {
                                IntegerLiteral::I8(v) => *v as i128,
                                IntegerLiteral::I16(v) => *v as i128,
                                IntegerLiteral::I32(v) => *v as i128,
                                IntegerLiteral::I64(v) => *v as i128,
                                IntegerLiteral::I128(v) => *v,
                                IntegerLiteral::U8(v) => *v as i128,
                                IntegerLiteral::U16(v) => *v as i128,
                                IntegerLiteral::U32(v) => *v as i128,
                                IntegerLiteral::U64(v) => *v as i128,
                                IntegerLiteral::U128(v) => *v as i128,
                            };

                            let mut target_bb = *otherwise;
                            for (v, bb) in targets {
                                if v.value() == int_val as u128 {
                                    target_bb = *bb;
                                    break;
                                }
                            }

                            term.kind = TerminatorKind::Goto { target: target_bb };
                            changed = true;
                        } else if let Literal::Boolean(b) = &c.literal {
                            // SwitchInt usage for boolean: 0=false, 1=true usually
                            let val = if *b { 1u128 } else { 0u128 };
                            let mut target_bb = *otherwise;
                            for (v, bb) in targets {
                                if v.value() == val {
                                    target_bb = *bb;
                                    break;
                                }
                            }
                            term.kind = TerminatorKind::Goto { target: target_bb };
                            changed = true;
                        }
                    }
                }
            }
        }

        changed
    }

    fn name(&self) -> &'static str {
        "Constant Propagation"
    }
}

fn resolve_operand(op: &mut Operand, constants: &HashMap<Local, Constant>) {
    // Check if op is a constant but via local lookup
    let op_clone = if let Operand::Copy(place) | Operand::Move(place) = op {
        if place.projection.is_empty() {
            constants.get(&place.local).cloned()
        } else {
            None
        }
    } else {
        None
    };

    if let Some(c) = op_clone {
        *op = Operand::Constant(Box::new(c));
    }
}

fn fold_binary(op: crate::mir::BinOp, lhs: &Constant, rhs: &Constant) -> Option<Constant> {
    use crate::mir::BinOp;

    // Extract values. Only handling integers for now.
    let l_val = get_int(lhs)?;
    let r_val = get_int(rhs)?;

    let res = match op {
        BinOp::Add => l_val.wrapping_add(r_val),
        BinOp::Sub => l_val.wrapping_sub(r_val),
        BinOp::Mul => l_val.wrapping_mul(r_val),
        BinOp::Div => {
            if r_val == 0 {
                return None;
            } else {
                l_val.wrapping_div(r_val)
            }
        }
        BinOp::Rem => {
            if r_val == 0 {
                return None;
            } else {
                l_val.wrapping_rem(r_val)
            }
        }
        BinOp::BitAnd => l_val & r_val,
        BinOp::BitOr => l_val | r_val,
        BinOp::BitXor => l_val ^ r_val,
        BinOp::Shl => l_val.wrapping_shl(r_val as u32),
        BinOp::Shr => l_val.wrapping_shr(r_val as u32),
        BinOp::Eq => {
            if l_val == r_val {
                1
            } else {
                0
            }
        }
        BinOp::Ne => {
            if l_val != r_val {
                1
            } else {
                0
            }
        }
        BinOp::Lt => {
            if l_val < r_val {
                1
            } else {
                0
            }
        }
        BinOp::Le => {
            if l_val <= r_val {
                1
            } else {
                0
            }
        }
        BinOp::Gt => {
            if l_val > r_val {
                1
            } else {
                0
            }
        }
        BinOp::Ge => {
            if l_val >= r_val {
                1
            } else {
                0
            }
        }
        BinOp::Offset => return None, // Pointer arithmetic
    };

    let ty = if matches!(
        op,
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
    ) {
        crate::ast::types::Type::new(TypeKind::Boolean, lhs.span)
    } else {
        lhs.ty.clone()
    };

    Some(Constant {
        span: lhs.span,
        ty,
        literal: if matches!(
            op,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        ) {
            Literal::Boolean(res != 0)
        } else {
            // Reconstruct a typed integer literal preserving the LHS type
            reconstruct_integer_literal(&lhs.literal, res)
        },
    })
}

fn fold_unary(op: crate::mir::UnOp, operand: &Constant) -> Option<Constant> {
    use crate::mir::UnOp;
    let val = get_int(operand)?;

    let res = match op {
        UnOp::Neg => (-(val as i64)) as i128,
        UnOp::Not => !val,
        UnOp::Await => return None,
    };

    Some(Constant {
        span: operand.span,
        ty: operand.ty.clone(),
        literal: reconstruct_integer_literal(&operand.literal, res),
    })
}

/// Reconstruct a typed integer literal from a computed `i128` value,
/// preserving the original integer width from the source literal.
fn reconstruct_integer_literal(source: &Literal, value: i128) -> Literal {
    match source {
        Literal::Integer(IntegerLiteral::I8(_)) => {
            Literal::Integer(IntegerLiteral::I8(value as i8))
        }
        Literal::Integer(IntegerLiteral::I16(_)) => {
            Literal::Integer(IntegerLiteral::I16(value as i16))
        }
        Literal::Integer(IntegerLiteral::I32(_)) => {
            Literal::Integer(IntegerLiteral::I32(value as i32))
        }
        Literal::Integer(IntegerLiteral::I64(_)) => {
            Literal::Integer(IntegerLiteral::I64(value as i64))
        }
        Literal::Integer(IntegerLiteral::I128(_)) => Literal::Integer(IntegerLiteral::I128(value)),
        Literal::Integer(IntegerLiteral::U8(_)) => {
            Literal::Integer(IntegerLiteral::U8(value as u8))
        }
        Literal::Integer(IntegerLiteral::U16(_)) => {
            Literal::Integer(IntegerLiteral::U16(value as u16))
        }
        Literal::Integer(IntegerLiteral::U32(_)) => {
            Literal::Integer(IntegerLiteral::U32(value as u32))
        }
        Literal::Integer(IntegerLiteral::U64(_)) => {
            Literal::Integer(IntegerLiteral::U64(value as u64))
        }
        Literal::Integer(IntegerLiteral::U128(_)) => {
            Literal::Integer(IntegerLiteral::U128(value as u128))
        }
        // Non-integer source (e.g. Boolean used in arithmetic): default to I64
        _ => Literal::Integer(IntegerLiteral::I64(value as i64)),
    }
}

fn get_int(c: &Constant) -> Option<i128> {
    match &c.literal {
        Literal::Integer(lit) => Some(match lit {
            IntegerLiteral::I8(v) => *v as i128,
            IntegerLiteral::I16(v) => *v as i128,
            IntegerLiteral::I32(v) => *v as i128,
            IntegerLiteral::I64(v) => *v as i128,
            IntegerLiteral::I128(v) => *v,
            IntegerLiteral::U8(v) => *v as i128,
            IntegerLiteral::U16(v) => *v as i128,
            IntegerLiteral::U32(v) => *v as i128,
            IntegerLiteral::U64(v) => *v as i128,
            IntegerLiteral::U128(v) => *v as i128,
        }),
        Literal::Boolean(b) => Some(if *b { 1 } else { 0 }),
        _ => None,
    }
}
