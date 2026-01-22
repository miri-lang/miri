// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Core evaluation logic for the MIR interpreter.

use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::error::InterpreterError;

use crate::interpreter::value::Value;
use crate::interpreter::Interpreter;
use crate::mir::{
    AggregateKind, BinOp, Body, Operand, Place, PlaceElem, Rvalue, StatementKind, TerminatorKind,
    UnOp,
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
    // Track previous block for Phi nodes
    let mut prev_block: Option<BasicBlock> = None;

    loop {
        context.previous_block = prev_block;

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
                TerminatorResult::Goto(target) => {
                    prev_block = Some(current_block);
                    current_block = target;
                }
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
    previous_block: Option<BasicBlock>,
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
            previous_block: None,
        }
    }

    fn execute_statement(&mut self, stmt: &Statement) -> Result<(), InterpreterError> {
        match &stmt.kind {
            StatementKind::Assign(place, rvalue) => {
                let value = self.eval_rvalue(rvalue)?;
                self.write_place(place, value)?;
            }
            StatementKind::Nop => {}
            StatementKind::StorageLive(place) => {
                // Mark local as live but uninitialized
                if place.projection.is_empty() {
                    self.locals.insert(place.local, Value::Uninitialized);
                }
            }
            StatementKind::StorageDead(place) => {
                // Mark local as dead (remove it)
                if place.projection.is_empty() {
                    self.locals.remove(&place.local);
                }
            }
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
                    if int_val == match_val.value() as i128 {
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
            TerminatorKind::GpuLaunch {
                kernel: _,
                grid: _,
                block: _,
                destination: _,
                target,
            } => {
                // Simulate GPU launch by just proceeding (no-op for now)
                // In a real simulator, we would spawn threads.
                // Here we just warn and continue.
                eprintln!("warning: GPU launch simulated as no-op on CPU interpreter");

                if let Some(target_block) = target {
                    Ok(TerminatorResult::Goto(*target_block))
                } else {
                    Err(InterpreterError::internal(
                        "GpuLaunch terminator missing target",
                    ))
                }
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
            Rvalue::Ref(place) => {
                let value = self.read_place(place)?;
                Ok(Value::Ref(Box::new(value)))
            }
            Rvalue::Len(place) => {
                let value = self.read_place(place)?;
                match value {
                    Value::Array(arr) => Ok(Value::Int(arr.len() as i128)),
                    Value::String(s) => Ok(Value::Int(s.len() as i128)),
                    Value::Tuple(t) => Ok(Value::Int(t.len() as i128)),
                    Value::Map(m) => Ok(Value::Int(m.len() as i128)),
                    _ => Err(InterpreterError::type_mismatch(
                        "array, string, tuple, or map",
                        value.type_name(),
                        "len",
                    )),
                }
            }
            Rvalue::Cast(op, ty) => {
                let value = self.eval_operand(op)?;
                // Simple cast logic for interpreter
                match (value, &ty.kind) {
                    (Value::Int(v), TypeKind::Float) | (Value::Int(v), TypeKind::F64) => {
                        Ok(Value::Float(v as f64))
                    }
                    (Value::Int(v), TypeKind::F32) => Ok(Value::Float(v as f64)), // Using f64 for all floats in Value?
                    (Value::Float(v), TypeKind::Int)
                    | (Value::Float(v), TypeKind::I32)
                    | (Value::Float(v), TypeKind::I64) => Ok(Value::Int(v as i128)),
                    (Value::Float(v), TypeKind::I8) | (Value::Float(v), TypeKind::U8) => {
                        Ok(Value::Int(v as i128))
                    }
                    // Int to Int casts (truncation or extension) - Value::Int is i128 so it covers all
                    (Value::Int(v), _) if ty.is_copy() => Ok(Value::Int(v)),
                    // String/etc casts?
                    (v, _) => Ok(v), // Default to identity if not special
                }
            }
            Rvalue::Aggregate(kind, operands) => {
                // Evaluate all operands first
                let values: Vec<Value> = operands
                    .iter()
                    .map(|op| self.eval_operand(op))
                    .collect::<Result<_, _>>()?;

                match kind {
                    AggregateKind::Tuple => Ok(Value::Tuple(values)),
                    AggregateKind::Array | AggregateKind::List => Ok(Value::Array(values)),
                    AggregateKind::Set => {
                        // Sets are represented as arrays for now
                        Ok(Value::Array(values))
                    }
                    AggregateKind::Map => {
                        // Values alternate: key1, val1, key2, val2...
                        let mut map = HashMap::new();
                        for chunk in values.chunks(2) {
                            if let [k, v] = chunk {
                                let key = match k {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                };
                                map.insert(key, v.clone());
                            }
                        }
                        Ok(Value::Map(map))
                    }
                    AggregateKind::Struct(ty) => {
                        let name = ty.to_string();
                        // For structs, we don't have field names here, so store by index
                        let mut fields = HashMap::new();
                        for (i, v) in values.into_iter().enumerate() {
                            fields.insert(i.to_string(), v);
                        }
                        Ok(Value::Struct(name, fields))
                    }
                    AggregateKind::Enum(type_name, variant_name) => {
                        // First operand is discriminant, rest are associated values
                        let associated = if values.len() > 1 {
                            Some(Box::new(Value::Tuple(values[1..].to_vec())))
                        } else {
                            None
                        };
                        Ok(Value::Enum(
                            type_name.clone(),
                            variant_name.clone(),
                            associated,
                        ))
                    }
                }
            }
            Rvalue::GpuIntrinsic(intrinsic) => {
                // Simulate GPU intrinsics with single-thread CPU values
                use crate::mir::rvalue::GpuIntrinsic;
                match intrinsic {
                    GpuIntrinsic::ThreadIdx(_) => Ok(Value::Int(0)),
                    GpuIntrinsic::BlockIdx(_) => Ok(Value::Int(0)),
                    GpuIntrinsic::BlockDim(_) => Ok(Value::Int(1)), // 1 thread per block
                    GpuIntrinsic::GridDim(_) => Ok(Value::Int(1)),  // 1 block per grid
                    GpuIntrinsic::SyncThreads => Ok(Value::None),
                }
            }
            Rvalue::Phi(args) => {
                // Find value for previous_block
                if let Some(prev) = self.previous_block {
                    for (op, block) in args {
                        if *block == prev {
                            return self.eval_operand(op);
                        }
                    }
                    // If not found, that's an issue with CFG or Phi node vs predecessor
                    // (Unless the previous block is not listed? e.g. dead code entry?)
                    // But assume valid MIR.
                    Err(InterpreterError::internal(format!(
                        "Phi node has no entry for previous block {:?}",
                        prev
                    )))
                } else {
                    // Entry block should not have Phi nodes usually?
                    // Or if we jumped from nowhere?
                    Err(InterpreterError::internal(
                        "Phi node evaluated without previous block",
                    ))
                }
            }
        }
    }

    fn eval_operand(&self, operand: &Operand) -> Result<Value, InterpreterError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.read_place(place),
            Operand::Constant(c) => eval_constant(&c.literal),
        }
    }

    fn read_place(&self, place: &Place) -> Result<Value, InterpreterError> {
        let mut value = self.read_local(place.local)?;

        // Apply each projection element to traverse into nested structures
        for proj in &place.projection {
            value = self.apply_projection(value, proj)?;
        }

        Ok(value)
    }

    fn read_local(&self, local: Local) -> Result<Value, InterpreterError> {
        let val = self.locals.get(&local).cloned().ok_or_else(|| {
            // Check if it should have been initialized (params or declared)
            // Just return uninitialized error
            InterpreterError::uninitialized_local(local.0)
        })?;

        if let Value::Uninitialized = val {
            return Err(InterpreterError::uninitialized_local(local.0));
        }

        Ok(val)
    }

    fn write_place(&mut self, place: &Place, value: Value) -> Result<(), InterpreterError> {
        if place.projection.is_empty() {
            self.locals.insert(place.local, value);
            return Ok(());
        }

        // For projected writes, get the root value, modify it, then put it back
        let mut root = self.read_local(place.local)?;
        self.write_projected(&mut root, &place.projection, value)?;
        self.locals.insert(place.local, root);
        Ok(())
    }

    fn write_projected(
        &self,
        value: &mut Value,
        projection: &[PlaceElem],
        new_val: Value,
    ) -> Result<(), InterpreterError> {
        if projection.is_empty() {
            *value = new_val;
            return Ok(());
        }

        let (first, rest) = projection.split_first().unwrap();

        match first {
            PlaceElem::Field(idx) => match value {
                Value::Struct(_name, fields) | Value::Class(_name, fields) => {
                    // Get the field name at index idx
                    if let Some((_key, field_val)) = fields.iter_mut().nth(*idx) {
                        if rest.is_empty() {
                            *field_val = new_val;
                        } else {
                            self.write_projected(field_val, rest, new_val)?;
                        }
                        Ok(())
                    } else {
                        // Field doesn't exist, try to insert by index as string key
                        let key = idx.to_string();
                        if rest.is_empty() {
                            fields.insert(key, new_val);
                        } else {
                            let mut nested = Value::None;
                            self.write_projected(&mut nested, rest, new_val)?;
                            fields.insert(key, nested);
                        }
                        Ok(())
                    }
                }
                Value::Tuple(elements) => {
                    if let Some(elem) = elements.get_mut(*idx) {
                        if rest.is_empty() {
                            *elem = new_val;
                        } else {
                            self.write_projected(elem, rest, new_val)?;
                        }
                        Ok(())
                    } else {
                        Err(InterpreterError::internal(format!(
                            "Tuple index {} out of bounds",
                            idx
                        )))
                    }
                }
                _ => Err(InterpreterError::type_mismatch(
                    "struct, class, or tuple",
                    value.type_name(),
                    "field write",
                )),
            },
            PlaceElem::Index(local) => {
                let idx_val = self.read_local(*local)?;
                let idx = idx_val.as_int().ok_or_else(|| {
                    InterpreterError::type_mismatch("integer", idx_val.type_name(), "index")
                })? as usize;

                match value {
                    Value::Array(arr) => {
                        if let Some(elem) = arr.get_mut(idx) {
                            if rest.is_empty() {
                                *elem = new_val;
                            } else {
                                self.write_projected(elem, rest, new_val)?;
                            }
                            Ok(())
                        } else {
                            Err(InterpreterError::internal(format!(
                                "Array index {} out of bounds",
                                idx
                            )))
                        }
                    }
                    Value::Tuple(tup) => {
                        if let Some(elem) = tup.get_mut(idx) {
                            if rest.is_empty() {
                                *elem = new_val;
                            } else {
                                self.write_projected(elem, rest, new_val)?;
                            }
                            Ok(())
                        } else {
                            Err(InterpreterError::internal(format!(
                                "Tuple index {} out of bounds",
                                idx
                            )))
                        }
                    }
                    _ => Err(InterpreterError::type_mismatch(
                        "array or tuple",
                        value.type_name(),
                        "index write",
                    )),
                }
            }
            PlaceElem::Deref => match value {
                Value::Ref(inner) => {
                    if rest.is_empty() {
                        **inner = new_val;
                    } else {
                        self.write_projected(inner, rest, new_val)?;
                    }
                    Ok(())
                }
                _ => Err(InterpreterError::type_mismatch(
                    "reference",
                    value.type_name(),
                    "dereference write",
                )),
            },
        }
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
            BinOp::Offset => Err(InterpreterError::not_implemented(
                "pointer offset (requires memory model upgrade)",
            )),
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
            UnOp::Await => {
                // For the synchronous interpreter, await is identity
                // (async execution model not yet supported)
                Ok(val)
            }
        }
    }

    // Helper for projections if needed, currently unused by read_place but good to keep structure
    #[allow(dead_code)]
    fn apply_projection(&self, value: Value, proj: &PlaceElem) -> Result<Value, InterpreterError> {
        match proj {
            PlaceElem::Field(idx) => match value {
                Value::Struct(_name, fields) | Value::Class(_name, fields) => {
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
