// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::type_error::TypeError;
use std::collections::HashMap;

#[derive(Debug)]
pub struct TypeChecker {
    types: HashMap<usize, Type>,
    errors: Vec<TypeError>,
}

struct Context {
    scopes: Vec<HashMap<String, Type>>,
    return_types: Vec<Type>,
}

impl Context {
    fn new() -> Self {
        Self { 
            scopes: vec![HashMap::new()],
            return_types: Vec::new(),
        }
    }
    
    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    
    fn exit_scope(&mut self) {
        self.scopes.pop();
    }
    
    fn define(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }
    
    fn resolve(&self, name: &str) -> Option<Type> {
        for scope in self.scopes.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty.clone());
            }
        }
        None
    }
}

impl TypeChecker {
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
            errors: Vec::new(),
        }
    }

    pub fn get_type(&self, id: usize) -> Option<&Type> {
        self.types.get(&id)
    }

    pub fn check(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        let mut context = Context::new();
        for statement in &program.body {
            self.check_statement(statement, &mut context);
        }
        
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(self.errors.clone())
        }
    }

    fn check_statement(&mut self, statement: &Statement, context: &mut Context) {
        match statement {
            Statement::Variable(decls, _) => {
                for decl in decls {
                    let inferred_type = if let Some(init) = &decl.initializer {
                        self.infer_expression(init, context)
                    } else if let Some(type_expr) = &decl.typ {
                        match self.extract_type_from_expression(type_expr) {
                            Ok(t) => t,
                            Err(msg) => {
                                self.errors.push(TypeError::new(msg, type_expr.span.clone()));
                                Type::Error
                            }
                        }
                    } else {
                        self.errors.push(TypeError::new(
                            format!("Cannot infer type for variable '{}'", decl.name),
                            0..0 // TODO: Need span for variable declaration
                        ));
                        Type::Error
                    };
                    
                    if let Some(type_expr) = &decl.typ {
                        if let Some(init) = &decl.initializer {
                            let declared_type = match self.extract_type_from_expression(type_expr) {
                                Ok(t) => t,
                                Err(_) => Type::Error,
                            };
                            if !self.are_compatible(&declared_type, &inferred_type) {
                                self.errors.push(TypeError::new(
                                    format!("Type mismatch for variable '{}': expected {:?}, got {:?}", decl.name, declared_type, inferred_type),
                                    init.span.clone()
                                ));
                            }
                        }
                    }

                    context.define(decl.name.clone(), inferred_type);
                }
            }
            Statement::Expression(expr) => {
                self.infer_expression(expr, context);
            }
            Statement::Block(stmts) => {
                context.enter_scope();
                for s in stmts {
                    self.check_statement(s, context);
                }
                context.exit_scope();
            }
            Statement::If(cond, then_block, else_block, _) => {
                let cond_type = self.infer_expression(cond, context);
                if cond_type != Type::Boolean {
                    self.errors.push(TypeError::new(
                        format!("If condition must be a boolean, got {:?}", cond_type),
                        cond.span.clone()
                    ));
                }
                self.check_statement(then_block, context);
                if let Some(else_stmt) = else_block {
                    self.check_statement(else_stmt, context);
                }
            }
            Statement::While(cond, body, _) => {
                let cond_type = self.infer_expression(cond, context);
                if cond_type != Type::Boolean {
                    self.errors.push(TypeError::new(
                        format!("While condition must be a boolean, got {:?}", cond_type),
                        cond.span.clone()
                    ));
                }
                self.check_statement(body, context);
            }
            Statement::Return(expr_opt) => {
                let expected_return_type = context.return_types.last().unwrap_or(&Type::Void).clone();
                
                let actual_return_type = if let Some(expr) = expr_opt {
                    self.infer_expression(expr, context)
                } else {
                    Type::Void
                };

                if !self.are_compatible(&expected_return_type, &actual_return_type) {
                    let span = if let Some(expr) = expr_opt {
                        expr.span.clone()
                    } else {
                        0..0 // TODO: Need span for return statement
                    };
                    self.errors.push(TypeError::new(
                        format!("Invalid return type: expected {:?}, got {:?}", expected_return_type, actual_return_type),
                        span
                    ));
                }
            }
            Statement::FunctionDeclaration(name, generics, params, return_type_expr, body, _) => {
                let func_type = Type::Function(generics.clone(), params.clone(), return_type_expr.clone());
                context.define(name.clone(), func_type);

                let return_type = if let Some(rt_expr) = return_type_expr {
                    match self.extract_type_from_expression(rt_expr) {
                        Ok(t) => t,
                        Err(msg) => {
                            self.errors.push(TypeError::new(msg, rt_expr.span.clone()));
                            Type::Error
                        }
                    }
                } else {
                    Type::Void
                };
                
                context.return_types.push(return_type);
                context.enter_scope();

                for param in params {
                    let param_type = match self.extract_type_from_expression(&param.typ) {
                        Ok(t) => t,
                        Err(msg) => {
                            self.errors.push(TypeError::new(msg, param.typ.span.clone()));
                            Type::Error
                        }
                    };
                    context.define(param.name.clone(), param_type);
                }

                self.check_statement(body, context);

                context.exit_scope();
                context.return_types.pop();
            }
            _ => {}
        }
    }

    fn infer_expression(&mut self, expr: &Expression, context: &mut Context) -> Type {
        let ty = match &expr.node {
            ExpressionKind::Literal(lit) => self.infer_literal(lit),
            ExpressionKind::Binary(left, op, right) => {
                let left_ty = self.infer_expression(left, context);
                let right_ty = self.infer_expression(right, context);
                match self.check_binary_op(&left_ty, op, &right_ty) {
                    Ok(t) => t,
                    Err(msg) => {
                        self.errors.push(TypeError::new(msg, expr.span.clone()));
                        Type::Error
                    }
                }
            }
            ExpressionKind::Logical(left, op, right) => {
                let left_ty = self.infer_expression(left, context);
                let right_ty = self.infer_expression(right, context);
                match self.check_binary_op(&left_ty, op, &right_ty) {
                    Ok(t) => t,
                    Err(msg) => {
                        self.errors.push(TypeError::new(msg, expr.span.clone()));
                        Type::Error
                    }
                }
            }
            ExpressionKind::Unary(op, operand) => {
                let expr_ty = self.infer_expression(operand, context);
                match self.check_unary_op(op, &expr_ty) {
                    Ok(t) => t,
                    Err(msg) => {
                        self.errors.push(TypeError::new(msg, expr.span.clone()));
                        Type::Error
                    }
                }
            }
            ExpressionKind::Identifier(name, _) => {
                if let Some(ty) = context.resolve(name) {
                    ty
                } else {
                    self.errors.push(TypeError::new(format!("Undefined variable: {}", name), expr.span.clone()));
                    Type::Error
                }
            }
            ExpressionKind::Assignment(lhs, _, rhs) => {
                let rhs_type = self.infer_expression(rhs, context);
                let lhs_type = match &**lhs {
                    LeftHandSideExpression::Identifier(id_expr) => {
                        if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                            if let Some(ty) = context.resolve(name) {
                                ty
                            } else {
                                self.errors.push(TypeError::new(format!("Undefined variable: {}", name), id_expr.span.clone()));
                                Type::Error
                            }
                        } else {
                            self.errors.push(TypeError::new("Invalid assignment target".to_string(), expr.span.clone()));
                            Type::Error
                        }
                    }
                    _ => rhs_type.clone(), // Skip complex LHS for now
                };

                if !self.are_compatible(&lhs_type, &rhs_type) {
                    self.errors.push(TypeError::new(
                        format!("Type mismatch in assignment: cannot assign {:?} to {:?}", rhs_type, lhs_type),
                        expr.span.clone()
                    ));
                }
                lhs_type
            },
            ExpressionKind::Call(func, args) => {
                let func_type = self.infer_expression(func, context);
                match func_type {
                    Type::Function(_, params, return_type_expr) => {
                        if args.len() != params.len() {
                            self.errors.push(TypeError::new(
                                format!("Incorrect number of arguments: expected {}, got {}", params.len(), args.len()),
                                expr.span.clone()
                            ));
                        }

                        for (i, arg) in args.iter().enumerate() {
                            let arg_type = self.infer_expression(arg, context);
                            if i < params.len() {
                                let param_type = match self.extract_type_from_expression(&params[i].typ) {
                                    Ok(t) => t,
                                    Err(_) => Type::Error,
                                };
                                if !self.are_compatible(&param_type, &arg_type) {
                                    self.errors.push(TypeError::new(
                                        format!("Type mismatch for argument {}: expected {:?}, got {:?}", i + 1, param_type, arg_type),
                                        arg.span.clone()
                                    ));
                                }
                            }
                        }

                        if let Some(rt_expr) = return_type_expr {
                             match self.extract_type_from_expression(&rt_expr) {
                                Ok(t) => t,
                                Err(_) => Type::Error,
                            }
                        } else {
                            Type::Void
                        }
                    }
                    Type::Error => Type::Error,
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("Expression is not callable: {:?}", func_type),
                            func.span.clone()
                        ));
                        Type::Error
                    }
                }
            }
            _ => Type::Int,
        };

        self.types.insert(expr.id, ty.clone());
        ty
    }

    fn infer_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Integer(_) => Type::Int,
            Literal::Float(f) => match f {
                FloatLiteral::F32(_) => Type::F32,
                FloatLiteral::F64(_) => Type::F64,
            },
            Literal::Boolean(_) => Type::Boolean,
            Literal::String(_) => Type::String,
            Literal::Symbol(_) => Type::Symbol,
            Literal::Regex(_) => Type::Custom("Regex".into(), None), // Placeholder
        }
    }

    fn check_binary_op(&self, left: &Type, op: &BinaryOp, right: &Type) -> Result<Type, String> {
        match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                if self.is_numeric(left) && self.is_numeric(right) {
                    if self.are_compatible(left, right) {
                        Ok(left.clone())
                    } else {
                        Err(format!("Type mismatch: {:?} and {:?} are not compatible for arithmetic operation", left, right))
                    }
                } else if matches!(op, BinaryOp::Add) && matches!(left, Type::String) && matches!(right, Type::String) {
                    Ok(Type::String)
                } else {
                    Err(format!("Invalid types for arithmetic operation: {:?} and {:?}", left, right))
                }
            }
            BinaryOp::Equal | BinaryOp::NotEqual | 
            BinaryOp::LessThan | BinaryOp::LessThanEqual | 
            BinaryOp::GreaterThan | BinaryOp::GreaterThanEqual => {
                if self.are_compatible(left, right) {
                    Ok(Type::Boolean)
                } else {
                    Err(format!("Type mismatch: cannot compare {:?} and {:?}", left, right))
                }
            }
            BinaryOp::And | BinaryOp::Or => {
                if matches!(left, Type::Boolean) && matches!(right, Type::Boolean) {
                    Ok(Type::Boolean)
                } else {
                    Err(format!("Logical operations require booleans, got {:?} and {:?}", left, right))
                }
            }
            BinaryOp::BitwiseAnd | BinaryOp::BitwiseOr | BinaryOp::BitwiseXor => {
                if matches!(left, Type::Int) && matches!(right, Type::Int) {
                    Ok(Type::Int)
                } else {
                    Err(format!("Invalid types for bitwise operation: {:?} and {:?}", left, right))
                }
            }
            _ => Ok(Type::Boolean)
        }
    }

    fn check_unary_op(&self, op: &UnaryOp, expr_type: &Type) -> Result<Type, String> {
        match op {
            UnaryOp::Negate | UnaryOp::Plus | UnaryOp::Decrement | UnaryOp::Increment => {
                if self.is_numeric(expr_type) {
                    Ok(expr_type.clone())
                } else {
                    Err(format!("Unary operator requires numeric type, got {:?}", expr_type))
                }
            }
            UnaryOp::Not => {
                if matches!(expr_type, Type::Boolean) {
                    Ok(Type::Boolean)
                } else {
                    Err(format!("Logical NOT requires boolean, got {:?}", expr_type))
                }
            }
            _ => Ok(expr_type.clone())
        }
    }

    fn is_numeric(&self, t: &Type) -> bool {
        matches!(t, Type::Int | Type::Float | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128 | 
                    Type::U8 | Type::U16 | Type::U32 | Type::U64 | Type::U128 | Type::F32 | Type::F64)
    }

    fn _is_string(&self, t: &Type) -> bool {
        matches!(t, Type::String)
    }

    fn are_compatible(&self, t1: &Type, t2: &Type) -> bool {
        // Strict equality for now
        t1 == t2
    }

    fn extract_type_from_expression(&self, expr: &Expression) -> Result<Type, String> {
        match &expr.node {
            ExpressionKind::Type(t, _) => Ok(*t.clone()),
            _ => Err("Expected type expression".to_string()),
        }
    }
}
