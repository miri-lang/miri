// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::ast::*;
use crate::type_error::TypeError;
use crate::syntax_error::Span;
use std::collections::HashMap;

/// The TypeChecker struct is responsible for validating the type safety of the program.
/// It traverses the AST, infers types for expressions, and ensures that operations
/// and assignments are performed on compatible types.
#[derive(Debug)]
pub struct TypeChecker {
    /// Maps expression IDs to their inferred types.
    types: HashMap<usize, Type>,
    /// Collects all type errors encountered during checking.
    errors: Vec<TypeError>,
}

/// Context holds the current state of the type checking process, including
/// variable scopes, return types for functions, and loop depth.
struct Context {
    scopes: Vec<HashMap<String, Type>>,
    return_types: Vec<Type>,
    loop_depth: usize,
}

impl Context {
    fn new() -> Self {
        Self { 
            scopes: vec![HashMap::new()],
            return_types: Vec::new(),
            loop_depth: 0,
        }
    }
    
    /// Enters a new scope (e.g., block, function).
    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    
    /// Exits the current scope.
    fn exit_scope(&mut self) {
        self.scopes.pop();
    }

    /// Increments loop depth when entering a loop.
    fn enter_loop(&mut self) {
        self.loop_depth += 1;
    }

    /// Decrements loop depth when exiting a loop.
    fn exit_loop(&mut self) {
        if self.loop_depth > 0 {
            self.loop_depth -= 1;
        }
    }
    
    /// Defines a variable in the current scope.
    fn define(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, ty);
        }
    }
    
    /// Resolves a variable name to its type, searching from the innermost scope outwards.
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

    /// Main entry point for type checking a program.
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
            Statement::Variable(decls, _) => self.check_variable_declaration(decls, context),
            Statement::Expression(expr) => { self.infer_expression(expr, context); },
            Statement::Block(stmts) => self.check_block(stmts, context),
            Statement::If(cond, then_block, else_block, _) => self.check_if(cond, then_block, else_block, context),
            Statement::While(cond, body, _) => self.check_while(cond, body, context),
            Statement::For(decls, iterable, body) => self.check_for(decls, iterable, body, context),
            Statement::Break => self.check_break(context),
            Statement::Continue => self.check_continue(context),
            Statement::Return(expr) => self.check_return(expr, context),
            Statement::FunctionDeclaration(name, generics, params, return_type, body, _) => 
                self.check_function_declaration(name, generics, params, return_type, body, context),
            _ => {}
        }
    }

    // --- Statement Checkers ---

    fn check_variable_declaration(&mut self, decls: &[VariableDeclaration], context: &mut Context) {
        for decl in decls {
            let inferred_type = self.determine_variable_type(decl, context);
            context.define(decl.name.clone(), inferred_type);
        }
    }

    fn determine_variable_type(&mut self, decl: &VariableDeclaration, context: &mut Context) -> Type {
        let inferred_type = if let Some(init) = &decl.initializer {
            self.infer_expression(init, context)
        } else if let Some(type_expr) = &decl.typ {
            self.resolve_type_expression(type_expr)
        } else {
            self.report_error(format!("Cannot infer type for variable '{}'", decl.name), 0..0); // TODO: Span
            Type::Error
        };

        // If both type annotation and initializer exist, check compatibility
        if let (Some(type_expr), Some(init)) = (&decl.typ, &decl.initializer) {
            let declared_type = self.resolve_type_expression(type_expr);
            if !self.are_compatible(&declared_type, &inferred_type) {
                self.report_error(
                    format!("Type mismatch for variable '{}': expected {:?}, got {:?}", decl.name, declared_type, inferred_type),
                    init.span.clone()
                );
            }
        }
        
        inferred_type
    }

    fn check_block(&mut self, stmts: &[Statement], context: &mut Context) {
        context.enter_scope();
        for s in stmts {
            self.check_statement(s, context);
        }
        context.exit_scope();
    }

    fn check_if(&mut self, cond: &Expression, then_block: &Statement, else_block: &Option<Box<Statement>>, context: &mut Context) {
        let cond_type = self.infer_expression(cond, context);
        if cond_type != Type::Boolean {
            self.report_error(format!("If condition must be a boolean, got {:?}", cond_type), cond.span.clone());
        }
        self.check_statement(then_block, context);
        if let Some(else_stmt) = else_block {
            self.check_statement(else_stmt, context);
        }
    }

    fn check_while(&mut self, cond: &Expression, body: &Statement, context: &mut Context) {
        let cond_type = self.infer_expression(cond, context);
        if cond_type != Type::Boolean {
            self.report_error(format!("While condition must be a boolean, got {:?}", cond_type), cond.span.clone());
        }
        context.enter_loop();
        self.check_statement(body, context);
        context.exit_loop();
    }

    fn check_for(&mut self, decls: &[VariableDeclaration], iterable: &Expression, body: &Statement, context: &mut Context) {
        let iterable_type = self.infer_expression(iterable, context);
        let element_type = self.get_iterable_element_type(&iterable_type, iterable.span.clone());
        
        context.enter_scope();
        context.enter_loop();

        self.bind_loop_variables(decls, &element_type, iterable.span.clone(), context);

        self.check_statement(body, context);
        context.exit_loop();
        context.exit_scope();
    }

    fn bind_loop_variables(&mut self, decls: &[VariableDeclaration], element_type: &Type, span: Span, context: &mut Context) {
        if decls.len() == 1 {
            let decl = &decls[0];
            let var_type = if let Some(type_expr) = &decl.typ {
                let declared_type = self.resolve_type_expression(type_expr);
                if !self.are_compatible(&declared_type, element_type) {
                     self.report_error(
                        format!("Type mismatch for loop variable '{}': expected {:?}, got {:?}", decl.name, declared_type, element_type),
                        type_expr.span.clone()
                    );
                }
                declared_type
            } else {
                element_type.clone()
            };
            context.define(decl.name.clone(), var_type);
        } else if decls.len() == 2 {
            if let Type::Tuple(exprs) = element_type {
                if exprs.len() == 2 {
                    let key_type = self.extract_type_from_expression(&exprs[0]).unwrap_or(Type::Error);
                    let val_type = self.extract_type_from_expression(&exprs[1]).unwrap_or(Type::Error);
                    
                    context.define(decls[0].name.clone(), key_type);
                    context.define(decls[1].name.clone(), val_type);
                } else {
                    self.report_error("Destructuring mismatch: expected tuple of size 2".to_string(), span);
                }
            } else {
                self.report_error(format!("Expected tuple for destructuring, got {:?}", element_type), span);
            }
        } else {
            self.report_error("Invalid number of loop variables".to_string(), span);
        }
    }

    fn check_break(&mut self, context: &Context) {
        if context.loop_depth == 0 {
            self.report_error("Break statement outside of loop".to_string(), 0..0);
        }
    }

    fn check_continue(&mut self, context: &Context) {
        if context.loop_depth == 0 {
            self.report_error("Continue statement outside of loop".to_string(), 0..0);
        }
    }

    fn check_return(&mut self, expr_opt: &Option<Box<Expression>>, context: &mut Context) {
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
            self.report_error(
                format!("Invalid return type: expected {:?}, got {:?}", expected_return_type, actual_return_type),
                span
            );
        }
    }

    fn check_function_declaration(&mut self, name: &str, generics: &Option<Vec<Expression>>, params: &[Parameter], return_type_expr: &Option<Box<Expression>>, body: &Statement, context: &mut Context) {
        let func_type = Type::Function(generics.clone(), params.to_vec(), return_type_expr.clone());
        context.define(name.to_string(), func_type);

        let return_type = if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr)
        } else {
            Type::Void
        };
        
        context.return_types.push(return_type);
        context.enter_scope();
        
        // Reset loop depth for function body as it's a new context
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        for param in params {
            let param_type = self.resolve_type_expression(&param.typ);
            context.define(param.name.clone(), param_type);
        }

        self.check_statement(body, context);

        context.loop_depth = old_loop_depth;
        context.exit_scope();
        context.return_types.pop();
    }

    // --- Expression Inference ---

    fn infer_expression(&mut self, expr: &Expression, context: &mut Context) -> Type {
        let ty = match &expr.node {
            ExpressionKind::Literal(lit) => self.infer_literal(lit),
            ExpressionKind::Binary(left, op, right) => self.infer_binary(left, op, right, expr.span.clone(), context),
            ExpressionKind::Logical(left, op, right) => self.infer_logical(left, op, right, expr.span.clone(), context),
            ExpressionKind::Unary(op, operand) => self.infer_unary(op, operand, expr.span.clone(), context),
            ExpressionKind::Identifier(name, _) => self.infer_identifier(name, expr.span.clone(), context),
            ExpressionKind::Assignment(lhs, _, rhs) => self.infer_assignment(lhs, rhs, expr.span.clone(), context),
            ExpressionKind::Call(func, args) => self.infer_call(func, args, expr.span.clone(), context),
            ExpressionKind::Range(start, end, kind) => self.infer_range(start, end, kind, expr.span.clone(), context),
            _ => Type::Int, // Default fallback for unimplemented expressions
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
            Literal::Regex(_) => Type::Custom("Regex".into(), None),
        }
    }

    fn infer_binary(&mut self, left: &Expression, op: &BinaryOp, right: &Expression, span: Span, context: &mut Context) -> Type {
        let left_ty = self.infer_expression(left, context);
        let right_ty = self.infer_expression(right, context);
        
        match self.check_binary_op_types(&left_ty, op, &right_ty) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                Type::Error
            }
        }
    }

    fn infer_logical(&mut self, left: &Expression, op: &BinaryOp, right: &Expression, span: Span, context: &mut Context) -> Type {
        // Logical ops are binary ops in this AST, but we can treat them similarly
        self.infer_binary(left, op, right, span, context)
    }

    fn infer_unary(&mut self, op: &UnaryOp, operand: &Expression, span: Span, context: &mut Context) -> Type {
        let expr_ty = self.infer_expression(operand, context);
        match self.check_unary_op_types(op, &expr_ty) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                Type::Error
            }
        }
    }

    fn infer_identifier(&mut self, name: &str, span: Span, context: &Context) -> Type {
        if let Some(ty) = context.resolve(name) {
            ty
        } else {
            self.report_error(format!("Undefined variable: {}", name), span);
            Type::Error
        }
    }

    fn infer_assignment(&mut self, lhs: &LeftHandSideExpression, rhs: &Expression, span: Span, context: &mut Context) -> Type {
        let rhs_type = self.infer_expression(rhs, context);
        let lhs_type = match lhs {
            LeftHandSideExpression::Identifier(id_expr) => {
                if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                    self.infer_identifier(name, id_expr.span.clone(), context)
                } else {
                    self.report_error("Invalid assignment target".to_string(), span.clone());
                    Type::Error
                }
            }
            _ => rhs_type.clone(), // Skip complex LHS for now
        };

        if !self.are_compatible(&lhs_type, &rhs_type) {
            self.report_error(
                format!("Type mismatch in assignment: cannot assign {:?} to {:?}", rhs_type, lhs_type),
                span
            );
        }
        lhs_type
    }

    fn infer_call(&mut self, func: &Expression, args: &[Expression], span: Span, context: &mut Context) -> Type {
        let func_type = self.infer_expression(func, context);
        match func_type {
            Type::Function(_, params, return_type_expr) => {
                if args.len() != params.len() {
                    self.report_error(
                        format!("Incorrect number of arguments: expected {}, got {}", params.len(), args.len()),
                        span.clone()
                    );
                }

                for (i, arg) in args.iter().enumerate() {
                    let arg_type = self.infer_expression(arg, context);
                    if i < params.len() {
                        let param_type = self.resolve_type_expression(&params[i].typ);
                        if !self.are_compatible(&param_type, &arg_type) {
                            self.report_error(
                                format!("Type mismatch for argument {}: expected {:?}, got {:?}", i + 1, param_type, arg_type),
                                arg.span.clone()
                            );
                        }
                    }
                }

                if let Some(rt_expr) = return_type_expr {
                     self.resolve_type_expression(&rt_expr)
                } else {
                    Type::Void
                }
            }
            Type::Error => Type::Error,
            _ => {
                self.report_error(format!("Expression is not callable: {:?}", func_type), func.span.clone());
                Type::Error
            }
        }
    }

    fn infer_range(&mut self, start: &Expression, end: &Option<Box<Expression>>, kind: &RangeExpressionType, span: Span, context: &mut Context) -> Type {
        let start_type = self.infer_expression(start, context);
        
        if matches!(kind, RangeExpressionType::IterableObject) {
            return start_type;
        }

        if let Some(end_expr) = end {
            let end_type = self.infer_expression(end_expr, context);
            if !self.are_compatible(&start_type, &end_type) {
                 self.report_error(
                    format!("Range types mismatch: {:?} and {:?}", start_type, end_type),
                    span
                );
            }
        }
        
        let type_expr = self.create_type_expression(start_type);
        Type::Custom("Range".to_string(), Some(vec![type_expr]))
    }

    // --- Helpers ---

    fn check_binary_op_types(&self, left: &Type, op: &BinaryOp, right: &Type) -> Result<Type, String> {
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

    fn check_unary_op_types(&self, op: &UnaryOp, expr_type: &Type) -> Result<Type, String> {
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

    fn are_compatible(&self, t1: &Type, t2: &Type) -> bool {
        // Strict equality for now
        t1 == t2
    }

    fn create_type_expression(&self, ty: Type) -> Expression {
        IdNode::new(0, ExpressionKind::Type(Box::new(ty), false), 0..0)
    }

    fn get_iterable_element_type(&mut self, ty: &Type, span: Span) -> Type {
        match ty {
            Type::List(inner) => self.extract_type_from_expression(inner).unwrap_or(Type::Error),
            Type::String => Type::String,
            Type::Set(inner) => self.extract_type_from_expression(inner).unwrap_or(Type::Error),
            Type::Map(key, val) => {
                Type::Tuple(vec![*key.clone(), *val.clone()])
            },
            Type::Custom(name, args) if name == "Range" => {
                 if let Some(args) = args {
                     if let Some(arg) = args.first() {
                         return self.extract_type_from_expression(arg).unwrap_or(Type::Error);
                     }
                 }
                 Type::Error
            }
            Type::Error => Type::Error,
            _ => {
                self.report_error(format!("Type {:?} is not iterable", ty), span);
                Type::Error
            }
        }
    }

    fn extract_type_from_expression(&self, expr: &Expression) -> Result<Type, String> {
        match &expr.node {
            ExpressionKind::Type(t, _) => Ok(*t.clone()),
            _ => Err("Expected type expression".to_string()),
        }
    }

    fn resolve_type_expression(&mut self, expr: &Expression) -> Type {
        match self.extract_type_from_expression(expr) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, expr.span.clone());
                Type::Error
            }
        }
    }

    fn report_error(&mut self, message: String, span: Span) {
        self.errors.push(TypeError::new(message, span));
    }
}
