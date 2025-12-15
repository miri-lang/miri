// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug)]
pub struct TypeChecker {
    types: HashMap<usize, Type>,
}

struct Context {
    scopes: Vec<HashMap<String, Type>>,
}

impl Context {
    fn new() -> Self {
        Self { scopes: vec![HashMap::new()] }
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
        }
    }

    pub fn get_type(&self, id: usize) -> Option<&Type> {
        self.types.get(&id)
    }

    pub fn check(&mut self, program: &Program) -> Result<(), String> {
        let mut context = Context::new();
        for statement in &program.body {
            self.check_statement(statement, &mut context)?;
        }
        Ok(())
    }

    fn check_statement(&mut self, statement: &Statement, context: &mut Context) -> Result<(), String> {
        match statement {
            Statement::Variable(decls, _) => {
                for decl in decls {
                    let inferred_type = if let Some(init) = &decl.initializer {
                        self.infer_expression(init, context)?
                    } else if let Some(type_expr) = &decl.typ {
                        self.extract_type_from_expression(type_expr)?
                    } else {
                        // If no initializer and no type, we can't infer (unless we allow 'any' or defer)
                        // For now, defaulting to Int to avoid blocking tests, or erroring?
                        // Let's error if we can't determine type.
                        return Err(format!("Cannot infer type for variable '{}'", decl.name));
                    };
                    
                    // If explicit type is provided, check if it matches initializer
                    if let Some(type_expr) = &decl.typ {
                        if let Some(init) = &decl.initializer {
                            let declared_type = self.extract_type_from_expression(type_expr)?;
                            let init_type = self.infer_expression(init, context)?;
                            if !self.are_compatible(&declared_type, &init_type) {
                                return Err(format!("Type mismatch for variable '{}': expected {:?}, got {:?}", decl.name, declared_type, init_type));
                            }
                        }
                    }

                    context.define(decl.name.clone(), inferred_type);
                }
                Ok(())
            }
            Statement::Expression(expr) => {
                self.infer_expression(expr, context)?;
                Ok(())
            }
            Statement::Block(stmts) => {
                context.enter_scope();
                for s in stmts {
                    self.check_statement(s, context)?;
                }
                context.exit_scope();
                Ok(())
            }
            Statement::If(cond, then_block, else_block, _) => {
                let cond_type = self.infer_expression(cond, context)?;
                if cond_type != Type::Boolean {
                    return Err(format!("If condition must be a boolean, got {:?}", cond_type));
                }
                self.check_statement(then_block, context)?;
                if let Some(else_stmt) = else_block {
                    self.check_statement(else_stmt, context)?;
                }
                Ok(())
            }
            Statement::While(cond, body, _) => {
                let cond_type = self.infer_expression(cond, context)?;
                if cond_type != Type::Boolean {
                    return Err(format!("While condition must be a boolean, got {:?}", cond_type));
                }
                self.check_statement(body, context)?;
                Ok(())
            }
            Statement::Return(Some(expr)) => {
                self.infer_expression(expr, context)?;
                Ok(())
            }
            _ => Ok(())
        }
    }

    fn infer_expression(&mut self, expr: &Expression, context: &mut Context) -> Result<Type, String> {
        let ty = match &expr.node {
            ExpressionKind::Literal(lit) => Ok(self.infer_literal(lit)),
            ExpressionKind::Binary(left, op, right) => {
                let left_ty = self.infer_expression(left, context)?;
                let right_ty = self.infer_expression(right, context)?;
                self.check_binary_op(&left_ty, op, &right_ty)
            }
            ExpressionKind::Logical(left, op, right) => {
                let left_ty = self.infer_expression(left, context)?;
                let right_ty = self.infer_expression(right, context)?;
                self.check_binary_op(&left_ty, op, &right_ty)
            }
            ExpressionKind::Unary(op, expr) => {
                let expr_ty = self.infer_expression(expr, context)?;
                self.check_unary_op(op, &expr_ty)
            }
            ExpressionKind::Identifier(name, _) => {
                context.resolve(name).ok_or_else(|| format!("Undefined variable: {}", name))
            }
            ExpressionKind::Assignment(lhs, _, rhs) => {
                let rhs_type = self.infer_expression(rhs, context)?;
                let lhs_type = match &**lhs {
                    LeftHandSideExpression::Identifier(id_expr) => {
                        if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                            context.resolve(name).ok_or_else(|| format!("Undefined variable: {}", name))?
                        } else {
                            return Err("Invalid assignment target".to_string());
                        }
                    }
                    _ => return Ok(rhs_type), // Skip complex LHS for now
                };

                if !self.are_compatible(&lhs_type, &rhs_type) {
                    return Err(format!("Type mismatch in assignment: cannot assign {:?} to {:?}", rhs_type, lhs_type));
                }
                Ok(lhs_type)
            },
            _ => Ok(Type::Int),
        }?;

        self.types.insert(expr.id, ty.clone());
        Ok(ty)
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

    fn is_string(&self, t: &Type) -> bool {
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
