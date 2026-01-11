// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

//! Core evaluation logic for the MIR interpreter.

use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::error::InterpreterError;
use crate::interpreter::frame::Frame;
use crate::interpreter::value::Value;
use crate::interpreter::Interpreter;
use crate::mir::{
    BinOp, Body, Operand, Place, PlaceElem, Rvalue, StatementKind, TerminatorKind, UnOp,
};

/// Execute a function body with the given arguments.
pub fn execute_function(
    interp: &mut Interpreter,
    body: &Body,
    args: Vec<Value>,
) -> Result<Value, InterpreterError> {
    let mut frame = Frame::new(body, args);

    loop {
        // Get current block
        let block_idx = frame.current_block.0;
        let block = body
            .basic_blocks
            .get(block_idx)
            .ok_or(InterpreterError::InvalidBlock(block_idx))?;

        // Execute statements
        while frame.stmt_index < block.statements.len() {
            let stmt = &block.statements[frame.stmt_index];
            execute_statement(&mut frame, &stmt.kind)?;
            frame.next_stmt();
        }

        // Execute terminator
        let terminator = block
            .terminator
            .as_ref()
            .ok_or(InterpreterError::Internal("Block has no terminator".into()))?;

        match execute_terminator(interp, &mut frame, body, &terminator.kind)? {
            ControlFlow::Continue => {
                // Jump already handled, continue loop
            }
            ControlFlow::Return => {
                return Ok(frame.get_return_value());
            }
        }
    }
}

/// Control flow after terminator execution.
enum ControlFlow {
    Continue,
    Return,
}

/// Execute a single statement.
fn execute_statement(frame: &mut Frame, kind: &StatementKind) -> Result<(), InterpreterError> {
    match kind {
        StatementKind::Assign(place, rvalue) => {
            let value = eval_rvalue(frame, rvalue)?;
            assign_to_place(frame, place, value)?;
        }
        StatementKind::StorageLive(_) | StatementKind::StorageDead(_) => {
            // These are hints, we can ignore them
        }
        StatementKind::Nop => {
            // Do nothing
        }
    }
    Ok(())
}

/// Execute a terminator and return control flow action.
fn execute_terminator(
    interp: &mut Interpreter,
    frame: &mut Frame,
    _body: &Body,
    kind: &TerminatorKind,
) -> Result<ControlFlow, InterpreterError> {
    match kind {
        TerminatorKind::Return => Ok(ControlFlow::Return),

        TerminatorKind::Goto { target } => {
            frame.goto(*target);
            Ok(ControlFlow::Continue)
        }

        TerminatorKind::SwitchInt {
            discr,
            targets,
            otherwise,
        } => {
            let value = eval_operand(frame, discr)?;
            let int_val = value
                .as_int()
                .ok_or_else(|| InterpreterError::TypeMismatch {
                    expected: "integer".into(),
                    got: format!("{:?}", value),
                    context: "switch discriminant".into(),
                })?;

            // Find matching target
            let target = targets
                .iter()
                .find(|(v, _)| *v == int_val as u128)
                .map(|(_, t)| *t)
                .unwrap_or(*otherwise);

            frame.goto(target);
            Ok(ControlFlow::Continue)
        }

        TerminatorKind::Call {
            func,
            args,
            destination,
            target,
        } => {
            // Evaluate function operand to get function name
            let func_name = match func {
                Operand::Constant(c) => match &c.literal {
                    Literal::Symbol(name) => name.clone(),
                    _ => {
                        return Err(InterpreterError::InvalidOperand {
                            operation: "call".into(),
                            operand: format!("{:?}", c),
                        })
                    }
                },
                _ => {
                    return Err(InterpreterError::NotImplemented(
                        "indirect function calls".into(),
                    ))
                }
            };

            // Handle built-in functions
            let result = match func_name.as_str() {
                "print" => {
                    let arg_values: Result<Vec<_>, _> =
                        args.iter().map(|a| eval_operand(frame, a)).collect();
                    for v in arg_values? {
                        println!("{}", v);
                    }
                    Value::None
                }
                _ => {
                    // Call user-defined function
                    let arg_values: Result<Vec<_>, _> =
                        args.iter().map(|a| eval_operand(frame, a)).collect();

                    // Get function body
                    let callee_body = interp
                        .get_function(&func_name)
                        .ok_or_else(|| InterpreterError::UndefinedFunction(func_name.clone()))?
                        .clone();

                    execute_function(interp, &callee_body, arg_values?)?
                }
            };

            // Store result
            assign_to_place(frame, destination, result)?;

            // Jump to continuation block
            if let Some(t) = target {
                frame.goto(*t);
            }
            Ok(ControlFlow::Continue)
        }

        TerminatorKind::Unreachable => Err(InterpreterError::Internal(
            "Reached unreachable code".into(),
        )),

        TerminatorKind::GpuLaunch { .. } => Err(InterpreterError::NotImplemented(
            "GPU kernel launch in interpreter".into(),
        )),
    }
}

/// Evaluate an rvalue to produce a value.
fn eval_rvalue(frame: &Frame, rvalue: &Rvalue) -> Result<Value, InterpreterError> {
    match rvalue {
        Rvalue::Use(operand) => eval_operand(frame, operand),

        Rvalue::BinaryOp(op, lhs, rhs) => {
            let lhs_val = eval_operand(frame, lhs)?;
            let rhs_val = eval_operand(frame, rhs)?;
            eval_binop(*op, lhs_val, rhs_val)
        }

        Rvalue::UnaryOp(op, operand) => {
            let val = eval_operand(frame, operand)?;
            eval_unop(*op, val)
        }

        Rvalue::Aggregate(kind, operands) => {
            let values: Result<Vec<_>, _> =
                operands.iter().map(|o| eval_operand(frame, o)).collect();
            eval_aggregate(kind, values?)
        }

        Rvalue::Ref(place) => {
            let val = read_place(frame, place)?;
            Ok(Value::Ref(Box::new(val)))
        }

        Rvalue::Cast(operand, _ty) => {
            // For now, just return the value unchanged
            // TODO: Implement proper type casting
            eval_operand(frame, operand)
        }

        Rvalue::Len(place) => {
            let val = read_place(frame, place)?;
            match val {
                Value::Array(arr) => Ok(Value::Int(arr.len() as i128)),
                Value::String(s) => Ok(Value::Int(s.len() as i128)),
                _ => Err(InterpreterError::TypeMismatch {
                    expected: "array or string".into(),
                    got: format!("{:?}", val),
                    context: "len operation".into(),
                }),
            }
        }

        Rvalue::GpuIntrinsic(_) => Err(InterpreterError::NotImplemented(
            "GPU intrinsics in interpreter".into(),
        )),
    }
}

/// Evaluate an operand.
fn eval_operand(frame: &Frame, operand: &Operand) -> Result<Value, InterpreterError> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => read_place(frame, place),
        Operand::Constant(constant) => eval_constant(&constant.literal),
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
        Literal::Regex(_) => Err(InterpreterError::NotImplemented("regex literals".into())),
    }
}

/// Read a value from a place.
fn read_place(frame: &Frame, place: &Place) -> Result<Value, InterpreterError> {
    let local_idx = place.local.0;
    let mut value = frame
        .get_local(local_idx)
        .cloned()
        .ok_or(InterpreterError::UninitializedLocal(local_idx))?;

    // Handle projections
    for proj in &place.projection {
        value = apply_projection(frame, value, proj)?;
    }

    Ok(value)
}

/// Apply a projection to a value.
fn apply_projection(
    frame: &Frame,
    value: Value,
    proj: &PlaceElem,
) -> Result<Value, InterpreterError> {
    match proj {
        PlaceElem::Field(idx) => {
            match value {
                Value::Struct(_name, fields) => {
                    // Field index - find by position
                    fields.values().nth(*idx).cloned().ok_or_else(|| {
                        InterpreterError::Internal(format!("Field {} not found", idx))
                    })
                }
                Value::Tuple(elements) => elements.get(*idx).cloned().ok_or_else(|| {
                    InterpreterError::Internal(format!("Tuple index {} out of bounds", idx))
                }),
                _ => Err(InterpreterError::TypeMismatch {
                    expected: "struct or tuple".into(),
                    got: format!("{:?}", value),
                    context: "field access".into(),
                }),
            }
        }
        PlaceElem::Index(local) => {
            let idx_val = frame
                .get_local(local.0)
                .ok_or(InterpreterError::UninitializedLocal(local.0))?;
            let idx = idx_val
                .as_int()
                .ok_or_else(|| InterpreterError::TypeMismatch {
                    expected: "integer".into(),
                    got: format!("{:?}", idx_val),
                    context: "index".into(),
                })? as usize;

            match value {
                Value::Array(arr) => arr.get(idx).cloned().ok_or_else(|| {
                    InterpreterError::Internal(format!("Index {} out of bounds", idx))
                }),
                Value::Tuple(tup) => tup.get(idx).cloned().ok_or_else(|| {
                    InterpreterError::Internal(format!("Index {} out of bounds", idx))
                }),
                _ => Err(InterpreterError::TypeMismatch {
                    expected: "array or tuple".into(),
                    got: format!("{:?}", value),
                    context: "index access".into(),
                }),
            }
        }
        PlaceElem::Deref => match value {
            Value::Ref(inner) => Ok(*inner),
            _ => Err(InterpreterError::TypeMismatch {
                expected: "reference".into(),
                got: format!("{:?}", value),
                context: "dereference".into(),
            }),
        },
    }
}

/// Assign a value to a place.
fn assign_to_place(frame: &mut Frame, place: &Place, value: Value) -> Result<(), InterpreterError> {
    if !place.projection.is_empty() {
        return Err(InterpreterError::NotImplemented(
            "assignment to projected place".into(),
        ));
    }
    frame.set_local(place.local.0, value);
    Ok(())
}

/// Evaluate a binary operation.
fn eval_binop(op: BinOp, lhs: Value, rhs: Value) -> Result<Value, InterpreterError> {
    match op {
        BinOp::Add => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_add(b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "add".into(),
                operand: format!("{:?} + {:?}", a, b),
            }),
        },
        BinOp::Sub => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_sub(b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a - b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 - b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a - b as f64)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "sub".into(),
                operand: format!("{:?} - {:?}", a, b),
            }),
        },
        BinOp::Mul => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a.wrapping_mul(b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a * b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 * b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a * b as f64)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "mul".into(),
                operand: format!("{:?} * {:?}", a, b),
            }),
        },
        BinOp::Div => match (lhs, rhs) {
            (_, Value::Int(0)) => Err(InterpreterError::DivisionByZero),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a / b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 / b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / b as f64)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "div".into(),
                operand: format!("{:?} / {:?}", a, b),
            }),
        },
        BinOp::Rem => match (lhs, rhs) {
            (_, Value::Int(0)) => Err(InterpreterError::RemainderByZero),
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a % b)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "rem".into(),
                operand: format!("{:?} % {:?}", a, b),
            }),
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
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "bitand".into(),
                operand: format!("{:?} & {:?}", a, b),
            }),
        },
        BinOp::BitOr => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a | b)),
            (Value::Bool(a), Value::Bool(b)) => Ok(Value::Bool(a || b)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "bitor".into(),
                operand: format!("{:?} | {:?}", a, b),
            }),
        },
        BinOp::BitXor => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a ^ b)),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "bitxor".into(),
                operand: format!("{:?} ^ {:?}", a, b),
            }),
        },
        BinOp::Shl => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a << (b as u32))),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "shl".into(),
                operand: format!("{:?} << {:?}", a, b),
            }),
        },
        BinOp::Shr => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a >> (b as u32))),
            (a, b) => Err(InterpreterError::InvalidOperand {
                operation: "shr".into(),
                operand: format!("{:?} >> {:?}", a, b),
            }),
        },
        BinOp::Offset => Err(InterpreterError::NotImplemented("pointer offset".into())),
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
        _ => Err(InterpreterError::InvalidOperand {
            operation: "comparison".into(),
            operand: format!("{:?} vs {:?}", a, b),
        }),
    }
}

/// Evaluate a unary operation.
fn eval_unop(op: UnOp, val: Value) -> Result<Value, InterpreterError> {
    match op {
        UnOp::Neg => match val {
            Value::Int(v) => Ok(Value::Int(-v)),
            Value::Float(v) => Ok(Value::Float(-v)),
            v => Err(InterpreterError::InvalidOperand {
                operation: "neg".into(),
                operand: format!("{:?}", v),
            }),
        },
        UnOp::Not => match val {
            Value::Bool(v) => Ok(Value::Bool(!v)),
            Value::Int(v) => Ok(Value::Int(!v)),
            v => Err(InterpreterError::InvalidOperand {
                operation: "not".into(),
                operand: format!("{:?}", v),
            }),
        },
        UnOp::Await => Err(InterpreterError::NotImplemented("await".into())),
    }
}

/// Evaluate an aggregate construction.
fn eval_aggregate(
    kind: &crate::mir::AggregateKind,
    values: Vec<Value>,
) -> Result<Value, InterpreterError> {
    use crate::mir::AggregateKind;
    match kind {
        AggregateKind::Tuple => Ok(Value::Tuple(values)),
        AggregateKind::Array | AggregateKind::List => Ok(Value::Array(values)),
        AggregateKind::Set => {
            // For simplicity, treat set as array (could dedupe later)
            Ok(Value::Array(values))
        }
        AggregateKind::Map => {
            // Alternating key-value pairs
            let mut map = std::collections::HashMap::new();
            for chunk in values.chunks(2) {
                if chunk.len() == 2 {
                    let key = match &chunk[0] {
                        Value::String(s) => s.clone(),
                        v => format!("{}", v),
                    };
                    map.insert(key, chunk[1].clone());
                }
            }
            Ok(Value::Map(map))
        }
        AggregateKind::Struct(ty) => {
            // Use type name for struct
            let name = format!("{}", ty);
            let mut field_map = std::collections::HashMap::new();
            for (i, value) in values.into_iter().enumerate() {
                field_map.insert(format!("field_{}", i), value);
            }
            Ok(Value::Struct(name, field_map))
        }
    }
}
