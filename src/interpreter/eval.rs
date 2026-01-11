// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Core evaluation logic for the MIR interpreter.

use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::error::InterpreterError;

use crate::interpreter::value::Value;
use crate::interpreter::Interpreter;
use crate::mir::{
    BinOp, Body, Operand, Place, PlaceElem, Rvalue, StatementKind, TerminatorKind, UnOp,
};

use crate::mir::{BasicBlock, Local, Statement, Terminator};
use std::collections::HashMap;

/// Execute a function body with the given arguments.
pub(crate) fn execute_function(
    interpreter: &mut Interpreter,
    body: &Body,
    args: Vec<Value>,
) -> Result<Value, InterpreterError> {
    let mut context = EvalContext::new(interpreter, body);

    // Map arguments to local variables (params are locals 1..n)
    if args.len() != body.arg_count {
        // Function arg count check
        // We don't have the function name easily available here, so generic context
        return Err(InterpreterError::type_mismatch(
            format!("{} arguments", body.arg_count),
            format!("{} arguments", args.len()),
            "function call argument count",
        ));
    }

    for (i, arg) in args.into_iter().enumerate() {
        context.locals.insert(Local(i + 1), arg);
    }

    // Initialize other locals to uninitialized (implicitly handled by map lookup failure)

    // Execute blocks starting from 0
    let mut current_block = BasicBlock(0);

    loop {
        let block_data = body
            .basic_blocks
            .get(current_block.0)
            .ok_or_else(|| InterpreterError::invalid_block(current_block.0))?;

        // Execute statements
        for stmt in &block_data.statements {
            context.execute_statement(stmt)?;
        }

        // Execute terminator
        if let Some(terminator) = &block_data.terminator {
            match context.execute_terminator(terminator)? {
                TerminatorResult::Goto(target) => current_block = target,
                TerminatorResult::Return(val) => return Ok(val),
            }
        } else {
            // Block fell through without terminator - should not happen in valid MIR
            return Err(InterpreterError::internal(
                "Basic block ended without terminator",
            ));
        }
    }
}

struct EvalContext<'a> {
    interpreter: &'a mut Interpreter,
    locals: HashMap<Local, Value>,
}

enum TerminatorResult {
    Goto(BasicBlock),
    Return(Value),
}

impl<'a> EvalContext<'a> {
    fn new(interpreter: &'a mut Interpreter, _body: &'a Body) -> Self {
        Self {
            interpreter,
            locals: HashMap::new(),
        }
    }

    fn execute_statement(&mut self, stmt: &Statement) -> Result<(), InterpreterError> {
        match &stmt.kind {
            StatementKind::Assign(place, rvalue) => {
                let value = self.eval_rvalue(rvalue)?;
                self.write_place(place, value)?;
            }
            StatementKind::Nop => {}
            StatementKind::StorageLive(_) | StatementKind::StorageDead(_) => {} // Ignore storage markers
        }
        Ok(())
    }

    fn execute_terminator(
        &mut self,
        terminator: &Terminator,
    ) -> Result<TerminatorResult, InterpreterError> {
        match &terminator.kind {
            TerminatorKind::Goto { target } => Ok(TerminatorResult::Goto(*target)),
            TerminatorKind::Return => {
                // Return value is in local 0
                let val = self.locals.remove(&Local(0)).unwrap_or(Value::None);
                Ok(TerminatorResult::Return(val))
            }
            TerminatorKind::SwitchInt {
                discr,
                targets,
                otherwise,
            } => {
                let val = self.eval_operand(discr)?;
                let int_val = val.as_int().ok_or_else(|| {
                    InterpreterError::type_mismatch(
                        "integer",
                        val.type_name(),
                        "switch discriminant",
                    )
                })?;

                for (match_val, target) in targets {
                    if int_val == *match_val as i128 {
                        return Ok(TerminatorResult::Goto(*target));
                    }
                }
                Ok(TerminatorResult::Goto(*otherwise))
            }
            TerminatorKind::Call {
                func,
                args,
                destination,
                target,
            } => {
                let func_val = self.eval_operand(func)?;
                let func_name = match func_val {
                    Value::String(s) => s,
                    _ => {
                        return Err(InterpreterError::type_mismatch(
                            "symbol",
                            func_val.type_name(),
                            "function call",
                        ));
                    }
                };

                // Evaluate arguments
                let mut arg_values = Vec::new();
                for arg in args {
                    arg_values.push(self.eval_operand(arg)?);
                }

                // Call the function
                let result = self.interpreter.call(&func_name, arg_values)?;

                // Store result
                self.write_place(destination, result)?;

                if let Some(target_block) = target {
                    Ok(TerminatorResult::Goto(*target_block))
                } else {
                    Err(InterpreterError::internal("Call terminator missing target"))
                }
            }
            TerminatorKind::Unreachable => {
                Err(InterpreterError::internal("Reached unreachable code"))
            }
            TerminatorKind::GpuLaunch { .. } => {
                Err(InterpreterError::not_implemented("GPU launch"))
            }
        }
    }

    fn eval_rvalue(&self, rvalue: &Rvalue) -> Result<Value, InterpreterError> {
        match rvalue {
            Rvalue::Use(op) => self.eval_operand(op),
            Rvalue::BinaryOp(op, lhs, rhs) => {
                let lhs_val = self.eval_operand(lhs)?;
                let rhs_val = self.eval_operand(rhs)?;
                self.eval_binop(*op, lhs_val, rhs_val)
            }
            Rvalue::UnaryOp(op, operand) => {
                let val = self.eval_operand(operand)?;
                self.eval_unop(*op, val)
            }
            Rvalue::Ref(_) => Err(InterpreterError::not_implemented("References")),
            Rvalue::Len(_) => Err(InterpreterError::not_implemented("Len intrinsic")),
            Rvalue::Cast(_, _) => Err(InterpreterError::not_implemented("Casts")),
            Rvalue::Aggregate(_, operands) => {
                // Primitive implementation: just return the first one if present (tuple checks omitted)
                if let Some(op) = operands.first() {
                    self.eval_operand(op)
                } else {
                    Ok(Value::None)
                }
            }
            Rvalue::GpuIntrinsic(_) => Err(InterpreterError::not_implemented("GPU intrinsics")),
        }
    }

    fn eval_operand(&self, operand: &Operand) -> Result<Value, InterpreterError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.read_place(place),
            Operand::Constant(c) => eval_constant(&c.literal),
        }
    }

    fn read_place(&self, place: &Place) -> Result<Value, InterpreterError> {
        // Only simple local access supported
        self.read_local(place.local)
    }

    fn read_local(&self, local: Local) -> Result<Value, InterpreterError> {
        self.locals.get(&local).cloned().ok_or_else(|| {
            // Check if it should have been initialized (params or declared)
            // Just return uninitialized error
            InterpreterError::uninitialized_local(local.0)
        })
    }

    fn write_place(&mut self, place: &Place, value: Value) -> Result<(), InterpreterError> {
        if !place.projection.is_empty() {
            return Err(InterpreterError::not_implemented(
                "assignment to projected place",
            ));
        }
        self.locals.insert(place.local, value);
        Ok(())
    }

    fn eval_binop(&self, op: BinOp, lhs: Value, rhs: Value) -> Result<Value, InterpreterError> {
        match op {
            BinOp::Add => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
                (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "add",
                    format!("{:?} + {:?}", a, b),
                )),
            },
            BinOp::Sub => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 - b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - b as f64)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "sub",
                    format!("{:?} - {:?}", a, b),
                )),
            },
            BinOp::Mul => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(b))),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "mul",
                    format!("{:?} * {:?}", a, b),
                )),
            },
            BinOp::Div => match (lhs, rhs) {
                (_, Value::Int(0)) => Err(InterpreterError::division_by_zero()),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 / b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / b as f64)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "div",
                    format!("{:?} / {:?}", a, b),
                )),
            },
            BinOp::Rem => match (lhs, rhs) {
                (_, Value::Int(0)) => Err(InterpreterError::remainder_by_zero()),
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "rem",
                    format!("{:?} % {:?}", a, b),
                )),
            },
            BinOp::Eq => Ok(Value::Bool(values_equal(&lhs, &rhs))),
            BinOp::Ne => Ok(Value::Bool(!values_equal(&lhs, &rhs))),
            BinOp::Lt => compare_values(&lhs, &rhs, |a, b| a < b, |a, b| a < b),
            BinOp::Le => compare_values(&lhs, &rhs, |a, b| a <= b, |a, b| a <= b),
            BinOp::Gt => compare_values(&lhs, &rhs, |a, b| a > b, |a, b| a > b),
            BinOp::Ge => compare_values(&lhs, &rhs, |a, b| a >= b, |a, b| a >= b),
            BinOp::BitAnd => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a & b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a && b)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "bitand",
                    format!("{:?} & {:?}", a, b),
                )),
            },
            BinOp::BitOr => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
                (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "bitor",
                    format!("{:?} | {:?}", a, b),
                )),
            },
            BinOp::BitXor => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "bitxor",
                    format!("{:?} ^ {:?}", a, b),
                )),
            },
            BinOp::Shl => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a << (b as u32))),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "shl",
                    format!("{:?} << {:?}", a, b),
                )),
            },
            BinOp::Shr => match (lhs, rhs) {
                (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a >> (b as u32))),
                (a, b) => Err(InterpreterError::invalid_operand(
                    "shr",
                    format!("{:?} >> {:?}", a, b),
                )),
            },
            BinOp::Offset => Err(InterpreterError::not_implemented("pointer offset")),
        }
    }

    fn eval_unop(&self, op: UnOp, val: Value) -> Result<Value, InterpreterError> {
        match op {
            UnOp::Neg => match val {
                Value::Int(v) => Ok(Value::Int(-v)),
                Value::Float(v) => Ok(Value::Float(-v)),
                v => Err(InterpreterError::invalid_operand("neg", format!("{:?}", v))),
            },
            UnOp::Not => match val {
                Value::Bool(v) => Ok(Value::Bool(!v)),
                Value::Int(v) => Ok(Value::Int(!v)),
                v => Err(InterpreterError::invalid_operand("not", format!("{:?}", v))),
            },
            _ => Err(InterpreterError::not_implemented(format!(
                "Unary operator {:?}",
                op
            ))),
        }
    }

    // Helper for projections if needed, currently unused by read_place but good to keep structure
    #[allow(dead_code)]
    fn apply_projection(&self, value: Value, proj: &PlaceElem) -> Result<Value, InterpreterError> {
        match proj {
            PlaceElem::Field(idx) => match value {
                Value::Struct(_name, fields) => {
                    fields.values().nth(*idx).cloned().ok_or_else(|| {
                        InterpreterError::internal(format!("Field {} not found", idx))
                    })
                }
                Value::Tuple(elements) => elements.get(*idx).cloned().ok_or_else(|| {
                    InterpreterError::internal(format!("Tuple index {} out of bounds", idx))
                }),
                _ => Err(InterpreterError::type_mismatch(
                    "struct or tuple",
                    format!("{:?}", value),
                    "field access",
                )),
            },
            PlaceElem::Index(local) => {
                let idx_val = self.read_local(*local)?;
                let idx = idx_val.as_int().ok_or_else(|| {
                    InterpreterError::type_mismatch("integer", format!("{:?}", idx_val), "index")
                })? as usize;

                match value {
                    Value::Array(arr) => arr.get(idx).cloned().ok_or_else(|| {
                        InterpreterError::internal(format!("Index {} out of bounds", idx))
                    }),
                    Value::Tuple(tup) => tup.get(idx).cloned().ok_or_else(|| {
                        InterpreterError::internal(format!("Index {} out of bounds", idx))
                    }),
                    _ => Err(InterpreterError::type_mismatch(
                        "array or tuple",
                        format!("{:?}", value),
                        "index access",
                    )),
                }
            }
            PlaceElem::Deref => match value {
                Value::Ref(inner) => Ok(*inner),
                _ => Err(InterpreterError::type_mismatch(
                    "reference",
                    format!("{:?}", value),
                    "dereference",
                )),
            },
        }
    }
}

/// Evaluate a constant literal.
fn eval_constant(literal: &Literal) -> Result<Value, InterpreterError> {
    match literal {
        Literal::Integer(int_lit) => {
            let val = match int_lit {
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
            Ok(Value::Int(val))
        }
        Literal::Float(float_lit) => {
            let val = match float_lit {
                FloatLiteral::F32(bits) => f32::from_bits(*bits) as f64,
                FloatLiteral::F64(bits) => f64::from_bits(*bits),
            };
            Ok(Value::Float(val))
        }
        Literal::Boolean(v) => Ok(Value::Bool(*v)),
        Literal::String(s) => Ok(Value::String(s.clone())),
        Literal::Symbol(s) => Ok(Value::String(s.clone())),
        Literal::None => Ok(Value::None),
        Literal::Regex(_) => Err(InterpreterError::not_implemented("regex literals")),
    }
}

/// Check if two values are equal.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::Float(x), Value::Float(y)) => x == y,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::None, Value::None) => true,
        _ => false,
    }
}

/// Compare two values.
fn compare_values<F, G>(
    a: &Value,
    b: &Value,
    int_cmp: F,
    float_cmp: G,
) -> Result<Value, InterpreterError>
where
    F: Fn(i128, i128) -> bool,
    G: Fn(f64, f64) -> bool,
{
    match (a, b) {
        (Value::Int(x), Value::Int(y)) => Ok(Value::Bool(int_cmp(*x, *y))),
        (Value::Float(x), Value::Float(y)) => Ok(Value::Bool(float_cmp(*x, *y))),
        (Value::Int(x), Value::Float(y)) => Ok(Value::Bool(float_cmp(*x as f64, *y))),
        (Value::Float(x), Value::Int(y)) => Ok(Value::Bool(float_cmp(*x, *y as f64))),
        _ => Err(InterpreterError::invalid_operand(
            "comparison",
            format!("{:?} vs {:?}", a, b),
        )),
    }
}
