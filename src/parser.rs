// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use crate::lexer::{token_to_string, Lexer, Token, TokenSpan};
use crate::syntax_error::{Span, SyntaxError, SyntaxErrorKind};
use crate::ast::*;


pub struct Parser<'source> {
    lexer: &'source mut Lexer<'source>,
    source: &'source str,
    _lookahead: Option<TokenSpan>,
    _ast_factory: AstFactory,
}

impl<'source> Parser<'source> {
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str, ast_factory: AstFactory) -> Self {
        Parser {
            lexer,
            source,
            _lookahead: None,
            _ast_factory: ast_factory,
        }
    }

    pub fn parse(&mut self) -> Result<Program, SyntaxError> {
        self._lookahead = self.lexer.next().transpose()?;
        self.program()
    }

    /*
        Program
            : StatementList
            ;
    */
    fn program(&mut self) -> Result<Program, SyntaxError> {
        let statements = self.statement_list()?;
        Ok(self._ast_factory.create_program(statements))
    }

    /*
        StatementList
            : Statement
            | StatementList Statement
            ;
    */
    fn statement_list(&mut self) -> Result<Vec<Statement>, SyntaxError> {
        let mut statements = vec![self.statement()?];

        // Continue parsing statements until we hit a Dedent or end of input
        while self._lookahead.is_some() && self._lookahead.as_ref().unwrap().0 != Token::Dedent {
            let statement = self.statement()?;
            statements.push(statement);
        }

        Ok(statements)
    }

    /*
        Statement
            : ExpressionStatement
            | BlockStatement
            | VariableStatement
            | IfStatement
            | WhileStatement
            | ForStatement
            | ForeverStatement
            | FunctionDeclaration
            | ReturnStatement
            | UseStatement
            | TypeStatement
            | BreakStatement
            | ContinueStatement
            | EnumStatement
            | StructStatement
            | EmptyStatement
            ;
    */
    fn statement(&mut self) -> Result<Statement, SyntaxError> {
        if self._lookahead.is_none() {
            return Ok(Statement::Empty);
        }

        let statement = match &self._lookahead {
            Some((Token::Public, _)) => {
                self.eat_token(&Token::Public)?;
                self.class_member_statement(MemberVisibility::Public)?
            },
            Some((Token::Protected, _)) => {
                self.eat_token(&Token::Protected)?;
                self.class_member_statement(MemberVisibility::Protected)?
            },
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                self.class_member_statement(MemberVisibility::Private)?
            },
            Some((Token::Indent, _)) => self.block_statement()?,
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement(MemberVisibility::Public)?,
            Some((Token::If, _)) => self.if_statement(IfStatementType::If)?,
            Some((Token::Unless, _)) => self.if_statement(IfStatementType::Unless)?,
            Some((Token::While, _)) => self.while_statement(WhileStatementType::While)?,
            Some((Token::Until, _)) => self.while_statement(WhileStatementType::Until)?,
            Some((Token::Forever, _)) => self.while_statement(WhileStatementType::Forever)?,
            Some((Token::For, _)) => self.for_statement()?,
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.function_declaration(MemberVisibility::Public)?
            }
            Some((Token::Return, _)) => self.return_statement()?,
            Some((Token::Use, _)) => self.use_statement()?,
            Some((Token::Type, _)) => self.type_statement(MemberVisibility::Public)?,
            Some((Token::Break, _)) => self.break_statement()?,
            Some((Token::Continue, _)) => self.continue_statement()?,
            Some((Token::Enum, _)) => self.enum_statement(MemberVisibility::Public)?,
            Some((Token::Struct, _)) => self.struct_statement(MemberVisibility::Public)?,
            _ => self.expression_statement()?,
        };
        Ok(statement)
    }

    fn class_member_statement(&mut self, visibility: MemberVisibility) -> Result<Statement, SyntaxError> {
        let statement = match &self._lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement(visibility)?,
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.function_declaration(visibility)?
            }
            Some((Token::Enum, _)) => self.enum_statement(visibility)?,
            Some((Token::Struct, _)) => self.struct_statement(visibility)?,
            Some((Token::Type, _)) => self.type_statement(visibility)?,
            _ => {
                return Err(self.error_unexpected_lookahead_token("let, var, async, def, gpu, enum, type or struct"));
            }
        };
        Ok(statement)
    }

    /*
        VariableStatement
            : 'let' VariableDeclarationList EXPRESSION_END
            | 'var' VariableDeclarationList EXPRESSION_END
            ;
    */
    fn variable_statement(&mut self, visibility: MemberVisibility) -> Result<Statement, SyntaxError> {
        let (token, variable_declaration_type) = match &self._lookahead {
            Some((Token::Let, _)) => (Token::Let, VariableDeclarationType::Immutable),
            Some((Token::Var, _)) => (Token::Var, VariableDeclarationType::Mutable),
            _ => Err(self.error_unexpected_lookahead_token("let or var"))?,
        };

        self.eat_token(&token)?;
        let declarations = self.variable_declaration_list(&variable_declaration_type, true)?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_variable_statement(declarations, visibility))
    }

    /*
        VariableDeclarationList
            : VariableDeclaration
            | VariableDeclarationList ',' VariableDeclaration
            ;
    */
    fn variable_declaration_list(&mut self, declaration_type: &VariableDeclarationType, accept_initializer: bool) -> Result<Vec<VariableDeclaration>, SyntaxError> {
        let mut declarations = vec![self.variable_declaration(declaration_type, accept_initializer)?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.variable_declaration(declaration_type, accept_initializer)?);
        }

        Ok(declarations)
    }

    fn parse_simple_identifier(&mut self) -> Result<String, SyntaxError> {
        let identifier_expr = self.identifier()?;
        if let Expression::Identifier(id, class_opt) = identifier_expr {
            if let Some(class) = class_opt {
                // A simple identifier cannot be namespaced.
                return Err(self.error_unexpected_token("a simple identifier", &format!("{}::{}", class, id)));
            }
            Ok(id)
        } else {
            // This case should ideally not be reachable if identifier() works correctly
            Err(self.error_unexpected_token("identifier", &format!("{:?}", identifier_expr)))
        }
    }

    /*
        VariableDeclaration
            : IDENTIFIER
            | IDENTIFIER TYPE
            | IDENTIFIER '=' Expression
            | IDENTIFIER TYPE '=' Expression
            ;
    */
    fn variable_declaration(&mut self, declaration_type: &VariableDeclarationType, accept_initializer: bool) -> Result<VariableDeclaration, SyntaxError> {
        let name = self.parse_simple_identifier()?;

        let typ = match self.type_expression()? {
            Some(typ) => Some(Box::new(typ)),
            None => None,
        };

        let initializer;
        if accept_initializer {
            initializer = match &self._lookahead {
                Some((Token::Assign, _)) => {
                    self.eat_token(&Token::Assign)?;
                    opt_expr(self.expression()?)
                },
                _ => None
            };
        }
        else {
            initializer = None;
        }

        Ok(VariableDeclaration {
            name,
            typ,
            initializer,
            declaration_type: declaration_type.clone(),
        })
    }

    fn statement_body(&mut self) -> Result<Statement, SyntaxError> {
        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;

            if self._lookahead.is_none() || self.lookahead_is_expression_end() || self.lookahead_is_dedent() || self.lookahead_is_else() {
                if self.lookahead_is_expression_end() {
                    self.eat_token(&Token::ExpressionStatementEnd)?;
                }
                // If the next token abruptly ends the statement, we can treat the body as empty
                return Ok(Statement::Empty);
            }

            return self.statement();
        }
        
        if self.lookahead_is_expression_end() {
            self.eat_token(&Token::ExpressionStatementEnd)?;

            if !self.lookahead_is_indent() {
                // If the next token abruptly ends the statement, we can treat the body as empty
                return Ok(Statement::Empty);
            }

            return self.statement();
        }

        return Err(self.error_unexpected_lookahead_token("a colon or an expression end"));
    }

    /*
        IfStatement
            : 'if' Expression ':' ExpressionStatement EXPRESSION_END ('else' ExpressionStatement EXPRESSION_END)?
            | 'if' Expression EXPRESSION_END BlockStatement ('else' EXPRESSION_END BlockStatement)?
            ;
    */
    fn if_statement(&mut self, if_statement_type: IfStatementType) -> Result<Statement, SyntaxError> {
        if if_statement_type == IfStatementType::Unless {
            self.eat_token(&Token::Unless)?;
        } else {
            self.eat_token(&Token::If)?;
        }
        let condition = self.expression()?;
        let then_block = self.statement_body()?;

        let else_block = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            if self.lookahead_is_colon() {
                let _ = self.eat_colon();
                if self._lookahead.is_none() {
                    None
                } else {
                    if self.lookahead_is_expression_end() {
                        // Empty else branch e.g. `else: // nothing`
                        let _ = self.eat_expression_end();
                        None
                    } else {
                        Some(self.statement()?)
                    }
                }
            } else {
                if self.lookahead_is_expression_end() {
                    let _ = self.eat_expression_end();
                    if self.lookahead_is_indent() {
                        Some(self.block_statement()?)
                    } else {
                        // No valid block after else
                        None
                    }
                } else {
                    Some(self.statement()?)
                }
            }
        } else {
            None
        };

        Ok(self._ast_factory.create_if_statement(condition, then_block, else_block, if_statement_type))
    }

    /*
        WhileStatement
            : 'while' Expression ':' ExpressionStatement EXPRESSION_END
            | 'while' Expression EXPRESSION_END BlockStatement
            | 'until' Expression ':' ExpressionStatement EXPRESSION_END
            | 'until' Expression EXPRESSION_END BlockStatement
            | 'forever' ':' ExpressionStatement EXPRESSION_END
            | 'forever' EXPRESSION_END BlockStatement
            ;
    */
    fn while_statement(&mut self, while_statement_type: WhileStatementType) -> Result<Statement, SyntaxError> {
        let condition;
        if while_statement_type == WhileStatementType::Until {
            self.eat_token(&Token::Until)?;
            condition = self.expression()?;
        } else if while_statement_type == WhileStatementType::Forever {
            self.eat_token(&Token::Forever)?;
            condition = self._ast_factory.create_literal_expression(
                self._ast_factory.create_boolean_literal(true)
            );
        } else {
            self.eat_token(&Token::While)?;
            condition = self.expression()?;
        }

        let then_block = self.statement_body()?;

        Ok(self._ast_factory.create_while_statement(condition, then_block, while_statement_type))
    }

    /*
        RangeBoundaryExpression
            : Identifier
            | StringLiteral
            | IntegerLiteral
            ;
    */
    fn range_boundary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let identifier = self.identifier()?;
                Ok(identifier)
            },
            _ => {
                let err = self.error_unexpected_lookahead_token("an identifier, a string or a number");
                if self.lookahead_is_literal() {
                    let literal = match &self._lookahead {
                        Some((Token::DoubleQuotedString, _)) => self.string_literal(&Token::DoubleQuotedString)?,
                        Some((Token::SingleQuotedString, _)) => self.string_literal(&Token::SingleQuotedString)?,
                        Some((Token::Int, _)) => self.integer_literal(&Token::Int)?,
                        Some((Token::BinaryNumber, _)) => self.integer_literal(&Token::BinaryNumber)?,
                        Some((Token::HexNumber, _)) => self.integer_literal(&Token::HexNumber)?,
                        Some((Token::OctalNumber, _)) => self.integer_literal(&Token::OctalNumber)?,
                        _ => return Err(err),
                    };
                    Ok(self._ast_factory.create_literal_expression(literal))
                } else {
                    Err(err)
                }
            }
        }
    }

    /*
        RangeExpression
            : RangeBoundaryExpression
            | RangeBoundaryExpression .. RangeBoundaryExpression
            | RangeBoundaryExpression ..= RangeBoundaryExpression
            ;
    */
    fn range_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.range_boundary_expression()?;
        let end: Option<Box<Expression>>;
        let range_type;

        match &self._lookahead {
            Some((Token::Range, _)) => {
                self.eat_token(&Token::Range)?;
                range_type = RangeExpressionType::Exclusive;
                end = opt_expr(self.range_boundary_expression()?);
            },
            Some((Token::RangeInclusive, _)) => {
                self.eat_token(&Token::RangeInclusive)?;
                range_type = RangeExpressionType::Inclusive;
                end = opt_expr(self.range_boundary_expression()?);
            },
            _ => {
                range_type = RangeExpressionType::IterableObject;
                end = None;
            }
        };

        Ok(self._ast_factory.create_range_expression(start, end, range_type))
    }

    /*
        ForStatement
            : 'for' VariableDeclarationList 'in' RangeExpression ':' ExpressionStatement EXPRESSION_END
            | 'for' VariableDeclarationList 'in' RangeExpression EXPRESSION_END BlockStatement
            ;
    */
    fn for_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::For)?;

        // For loop has immutable variable declarations without initializers
        let variable_declarations = self.variable_declaration_list(
            &VariableDeclarationType::Immutable,
            false
        )?;
        self.eat_token(&Token::In)?;
        let iterable = self.range_expression()?;

        let body = self.statement_body()?;

        Ok(self._ast_factory.create_for_statement(variable_declarations, iterable, body))
    }

    /*
        FunctionDeclaration
            : 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END BlockStatement
            | 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] ':' ExpressionStatement EXPRESSION_END
            ;
    */
    fn function_declaration(&mut self, visibility: MemberVisibility) -> Result<Statement, SyntaxError> {
        let mut properties = FunctionProperties {
            is_async: false,
            is_gpu: false,
            visibility,
        };

        while self.lookahead_is_function_modifier() {
            match &self._lookahead {
                Some((Token::Async, _)) => {
                    self.eat_token(&Token::Async)?;
                    properties.is_async = true;
                },
                Some((Token::Gpu, _)) => {
                    self.eat_token(&Token::Gpu)?;
                    properties.is_gpu = true;
                }
                _ => return Err(self.error_unexpected_lookahead_token("function modifier (async or gpu)")),
            }
        }

        self.eat_token(&Token::Fn)?;

        let name = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let token = self.eat_token(&Token::Identifier)?;
                self.source[token.1.start..token.1.end].to_string()
            },
            Some((Token::LessThan, _)) | Some((Token::LParen, _)) => {
                // No name, it's a lambda
                "".to_string()
            },
            _ => return Err(self.error_unexpected_lookahead_token("a function name, '(' or '<'")),
        };

        let generic_types = self.generic_types_expression()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        let body = if name.is_empty() {
            // This is a lambda expression. Its body parsing is special.
            if self.lookahead_is_colon() {
                self.eat_token(&Token::Colon)?;
                // An inline lambda body is a single expression, not a full statement.
                // We parse it and wrap it in an ExpressionStatement for the AST.
                let expr = self.expression()?;
                self._ast_factory.create_expression_statement(expr)
            } else {
                // A block lambda body is a normal block statement, which statement_body handles correctly.
                self.statement_body()?
            }
        } else {
            // This is a named function. Its body is always a full statement.
            self.statement_body()?
        };

        if name.is_empty() {
            return Ok(
                self._ast_factory.create_expression_statement(
                    self._ast_factory.create_lambda_expression(
                        generic_types,
                        parameters,
                        return_type,
                        body,
                        properties
                    )
                )
            );
        }

        Ok(self._ast_factory.create_function_declaration(name, generic_types, parameters, return_type, body, properties))
    }

    fn generic_types_expression(&mut self) -> Result<Option<Vec<Expression>>, SyntaxError> {
        let generic_types = if self.lookahead_is_less_than() {
            Some(self.generic_types_declaration()?)
        } else {
            None
        };
        Ok(generic_types)
    }

    fn function_params_expression(&mut self) -> Result<Vec<Parameter>, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let parameters =  if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.parameter_list()?
        };
        self.eat_token(&Token::RParen)?;

        Ok(parameters)
    }

    fn return_type_expression(&mut self) -> Result<Option<Box<Expression>>, SyntaxError> {
        let return_type = match self.type_expression()? {
            Some(typ) => Some(Box::new(typ)),
            None => None,
        };
        Ok(return_type)
    }

    /*
        GenericTypesDeclaration
            : '<' GenericType (',' GenericType)* '>'
            ;
    */
    fn generic_types_declaration(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(&Token::LessThan)?;

        let mut types = vec![self.generic_type()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            types.push(self.generic_type()?);
        }

        self.eat_token(&Token::GreaterThan)?;
        Ok(types)
    }

    /*
        GenericType
            : Identifier ('extends' | 'implements' | 'includes' TypeExpression)?
            ;
    */
    fn generic_type(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        if self._lookahead.is_none() || !self.lookahead_is_inheritance_modifier() {
            return Ok(self._ast_factory.create_generic_type_expression(identifier, None));
        }

        self.eat(is_inheritance_modifier, "extends, includes or implements")?;
        let typ = match self.type_expression()? {
            Some(typ) => Some(Box::new(typ)),
            None => None,
        };

        Ok(self._ast_factory.create_generic_type_expression(identifier, typ))
    }

    /*
        ParameterList
            : Parameter
            | ParameterList ',' Parameter
            ;
    */
    fn parameter_list(&mut self) -> Result<Vec<Parameter>, SyntaxError> {
        let mut parameters = vec![self.parameter()?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            parameters.push(self.parameter()?);
        }

        Ok(parameters)
    }

    /*
        Parameter
            : Identifier [TypeExpression] [Guard]
            ;
    */
    fn parameter(&mut self) -> Result<Parameter, SyntaxError> {
        let name = self.parse_simple_identifier()?;

        let typ = match self.type_expression()? {
            Some(typ) => Some(Box::new(typ)),
            None => None,
        };

        let guard = if self._lookahead.is_some() && self.lookahead_is_guard() {
            opt_expr(self.guard_expression()?)
        } else {
            None
        };

        Ok(Parameter { name, typ, guard })
    }

    /*
        GuardExpression
            : '>' Expression
            | '>=' Expression
            | '<' Expression
            | '<=' Expression
            | 'in' Expression
            | 'not' Expression
            | 'not in' Expression
            ;
    */
    fn guard_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut guard_op = match &self._lookahead {
            Some((Token::GreaterThan, _)) => GuardOp::GreaterThan,
            Some((Token::GreaterThanEqual, _)) => GuardOp::GreaterThanEqual,
            Some((Token::LessThan, _)) => GuardOp::LessThan,
            Some((Token::LessThanEqual, _)) => GuardOp::LessThanEqual,
            Some((Token::In, _)) => GuardOp::In,
            Some((Token::Not, _)) => GuardOp::Not,
            _ => return Err(self.error_unexpected_lookahead_token("guard operator")),
        };

        self.eat(is_guard, "guard operator")?;
        if self._lookahead.is_some() && self.lookahead_is_in() {
            self.eat_token(&Token::In)?;
            guard_op = GuardOp::NotIn;
        }

        let expression = self.expression()?;
        Ok(self._ast_factory.create_guard_expression(guard_op, expression))
    }

    /*
        ReturnStatement
            : 'return' Expression EXPRESSION_END
            ;
    */
    fn return_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Return)?;
        let expression = if self.lookahead_is_expression_end() {
            None
        } else {
            opt_expr(self.expression()?)
        };
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_return_statement(expression))
    }

    /*
        BreakStatement
            : 'break' EXPRESSION_END
            ;
    */
    fn break_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Break)?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_break_statement())
    }

    /*
        ContinueStatement
            : 'continue' EXPRESSION_END
            ;
    */
    fn continue_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Continue)?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_continue_statement())
    }

    fn parse_declaration_block<F, C>(
        &mut self,
        item_parser: F,
        creator: C,
        name: Expression,
        visibility: MemberVisibility,
        inline_error: &str,
        block_error: &str,
    ) -> Result<Statement, SyntaxError>
    where
        F: Fn(&mut Self) -> Result<Expression, SyntaxError>,
        C: Fn(&mut Self, Expression, Vec<Expression>, MemberVisibility) -> Statement,
    {
        let mut items = vec![];

        match &self._lookahead {
            Some((Token::Colon, _)) => { // Inline form
                self.eat_token(&Token::Colon)?;
                items.push(item_parser(self)?);
                while self.lookahead_is_comma() {
                    self.eat_token(&Token::Comma)?;
                    items.push(item_parser(self)?);
                }
                self.eat_expression_end()?;
            }
            Some((Token::ExpressionStatementEnd, _)) => { // Block form
                self.eat_expression_end()?;
                if !self.lookahead_is_indent() {
                    return Err(self.error_unexpected_lookahead_token(block_error));
                }
                self.eat_token(&Token::Indent)?;
                while !self.lookahead_is_dedent() {
                    items.push(item_parser(self)?);
                    self.eat_token(&Token::ExpressionStatementEnd)?;
                }
                self.eat_token(&Token::Dedent)?;
            },
            _ => return Err(self.error_unexpected_lookahead_token(inline_error)),
        };

        Ok(creator(self, name, items, visibility))
    }

    /*
        EnumStatement
            : 'enum' Identifier: EnumValue (',' EnumValue)*
            | 'enum' Identifier INDENT EnumValue EXPRESSION_END (EnumValue EXPRESSION_END)* DEDENT
            ;
    */
    fn enum_statement(&mut self, visibility: MemberVisibility) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Enum)?;
        let name = self.identifier()?;
        self.parse_declaration_block(
            Self::enum_value_expression,
            |p, n, vals: Vec<Expression>, vis| p._ast_factory.create_enum_statement(n, vals, vis),
            name,
            visibility,
            "either a colon for inline enums or an indentation for block enums",
            "an indentation for block enums",
        )
    }

    /*
        EnumValue
            : Identifier
            | Identifier '(' TypeExpression (',' TypeExpression)* ')'
            ;
    */
    pub fn enum_value_expression(&mut self) -> Result<Expression, SyntaxError> {
        let identifier = self.identifier()?;
        let types = if self.match_lookahead_type(|t| t == &Token::LParen) {
            self.multiple_element_type_expressions("Enum value type", &Token::LParen, &Token::RParen)?
        } else {
            vec![]
        };

        Ok(self._ast_factory.create_enum_value_expression(identifier, types))
    }

    /*
        StructStatement
            : 'struct' Identifier: StructMember (',' StructMember)*
            | 'struct' Identifier INDENT StructMember EXPRESSION_END (StructMember EXPRESSION_END)* DEDENT
            ;
    */
    fn struct_statement(&mut self, visibility: MemberVisibility) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Struct)?;
        let name = self.identifier()?;
        self.parse_declaration_block(
            Self::struct_member_expression,
            |p, n, m, vis| p._ast_factory.create_struct_statement(n, m, vis),
            name,
            visibility,
            "either a colon for inline structs or an indentation for block structs",
            "an indentation for block structs",
        )
    }

    /*
        StructMember
            : Identifier TypeExpression
            ;
    */
    fn struct_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let typ = self.type_expression()?;
        if typ.is_none() {
            return Err(self.error_missing_struct_member_type());
        }
        Ok(self._ast_factory.create_struct_member_expression(name, typ.unwrap()))
    }

    /*
        TypeExpression
            : Identifier ('<' TypeExpression ',' TypeExpression* '>')? '?'?
            | '[' TypeExpression ']' '?'?
            | '(' TypeExpression ',' TypeExpression* ')' '?'?
            | '{' TypeExpression '}' '?'?
            | '{' TypeExpression ':' TypeExpression* '}' '?'?
            ;
    */
    pub fn type_expression(&mut self) -> Result<Option<Expression>, SyntaxError> {
        if self._lookahead.is_none() {
            return Ok(None);
        }

        let typ: Type;
         
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let type_name = self.identifier_to_type_name()?;
                typ = self.type_name_to_type(type_name)?;
            },
            Some((Token::LBracket, _)) => {
                self.eat_token(&Token::LBracket)?;
                let element_type = self.element_type_expression("List element type")?;
                self.eat_token(&Token::RBracket)?;
                typ = Type::List(Box::new(element_type));
            },
            Some((Token::LParen, _)) => {
                let elements = self.multiple_element_type_expressions(
                    "Tuple element type", 
                    &Token::LParen, 
                    &Token::RParen
                )?;
                typ = Type::Tuple(elements);
            },
            Some((Token::LBrace, _)) => {
                self.eat_token(&Token::LBrace)?;
                let key_type = self.element_type_expression("Map key type")?;
                if self.match_lookahead_type(|t| t == &Token::Colon) {
                    self.eat_token(&Token::Colon)?;
                    let value_type = self.element_type_expression("Map value type")?;
                    self.eat_token(&Token::RBrace)?;
                    typ = Type::Map(Box::new(key_type), Box::new(value_type));
                } else {
                    self.eat_token(&Token::RBrace)?;
                    typ = Type::Set(Box::new(key_type));
                }
            },
            Some((Token::Fn, _)) => {
                self.eat_token(&Token::Fn)?;
                let generic_types = self.generic_types_expression()?;
                let parameters = self.function_params_expression()?;
                let return_type = self.return_type_expression()?;
                typ = Type::Function(generic_types, parameters, return_type);
            },
            _ => return Ok(None),
        }

        let is_nullable = if self.match_lookahead_type(|t| t == &Token::QuestionMark) {
            self.eat_token(&Token::QuestionMark)?;
            true
        } else {
            false
        };

        Ok(Some(self._ast_factory.create_type_expression(typ, is_nullable)))
    }

    fn type_name_to_type(&mut self, type_name: String) -> Result<Type, SyntaxError> {
        Ok(match type_name.as_str() {
            "int" => Type::Int,
            "i8" => Type::I8,
            "i16" => Type::I16,
            "i32" => Type::I32,
            "i64" => Type::I64,
            "i128" => Type::I128,
            "u8" => Type::U8,
            "u16" => Type::U16,
            "u32" => Type::U32,
            "u64" => Type::U64,
            "u128" => Type::U128,
            "float" => Type::Float,
            "f32" => Type::F32,
            "f64" => Type::F64,
            "string" => Type::String,
            "bool" => Type::Boolean,
            "symbol" => Type::Symbol,
            "result" => {
                self.generic_two_types_expression(
                    "Ok result type", 
                    "Error result type", 
                    |a, b| Type::Result(a, b)
                )?
            },
            "map" => {
                self.generic_two_types_expression(
                    "Map key type",
                    "Map value type",
                    |a, b| Type::Map(a, b)
                )?
            },
            "future" => {
                self.generic_one_type_expression(
                    "Future result type",
                    |inner| Type::Future(inner)
                )?
            }
            "list" => {
                self.generic_one_type_expression(
                    "List element type",
                    |inner| Type::List(inner)
                )?
            }
            "set" => {
                self.generic_one_type_expression(
                    "Set element type",
                    |inner| Type::Set(inner)
                )?
            },
            "tuple" => {
                let inner = self.multiple_element_type_expressions(
                    "Tuple item type",
                    &Token::LessThan,
                    &Token::GreaterThan
                )?;
                Type::Tuple(inner)
            }
            _ => {
                match &self._lookahead {
                    Some((Token::LessThan, _)) => {
                        let inner = self.multiple_element_type_expressions(
                            "Generic type",
                            &Token::LessThan,
                            &Token::GreaterThan
                        )?;
                        Type::Custom(type_name, Some(inner))
                    },
                    _ => Type::Custom(type_name, None),
                }
            },
        })
    }
    
    fn identifier_to_type_name(&mut self) -> Result<String, SyntaxError> {
        Ok(match self.identifier()? {
            Expression::Identifier(id, Some(class)) => format!("{}::{}", class, id), // Reconstruct the full path
            Expression::Identifier(id, None) => id,
            _ => return Err(
                self.error_unexpected_token("identifier", &self.lookahead_as_string())
            ),
        })
    }
    
    fn element_type_expression(&mut self, expected: &str) -> Result<Expression, SyntaxError> {
        let element_type = match self.type_expression()? {
            Some(typ) => typ,
            None => return Err(
                self.error_invalid_type_declaration(expected)
            ),
        };
        Ok(element_type)
    }

    fn multiple_element_type_expressions(&mut self, expected: &str, left_token: &Token, right_token: &Token) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(left_token)?;

        let element_type = self.element_type_expression(expected)?;
        let mut elements = vec![element_type];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            let element_type = self.element_type_expression(expected)?;
            elements.push(element_type);
        }

        self.eat_token(right_token)?;

        Ok(elements)
    }

    fn generic_one_type_expression<F>(&mut self, expected: &str, create_type: F) -> Result<Type, SyntaxError> 
        where F: FnOnce(Box<Expression>) -> Type
    {
        self.eat_token(&Token::LessThan)?;
        let inner_type = self.element_type_expression(expected)?;
        self.eat_token(&Token::GreaterThan)?;
        Ok(create_type(Box::new(inner_type)))
    }

    fn generic_two_types_expression<F>(&mut self, expected_a: &str, expected_b: &str, create_type: F) -> Result<Type, SyntaxError> 
        where F: FnOnce(Box<Expression>, Box<Expression>) -> Type
    {
        self.eat_token(&Token::LessThan)?;
        let a_type = self.element_type_expression(expected_a)?;
        self.eat_token(&Token::Comma)?;
        let b_type = self.element_type_expression(expected_b)?;
        self.eat_token(&Token::GreaterThan)?;

        Ok(create_type(Box::new(a_type), Box::new(b_type)))
    }

    /*
        UseStatement
            : 'use' ImportPathExpression ( 'as' Identifier )?
            ;
    */
    fn use_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Use)?;
        let import_path = self.import_path_expression()?;
        let alias = if self.match_lookahead_type(|t| t == &Token::As) {
            self.eat_token(&Token::As)?;
            opt_expr(self.identifier()?)
        } else {
            None
        };
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_use_statement(import_path, alias))
    }


    /*
        TypeStatement
            : 'type' TypeDeclaration, (',' TypeDeclaration)*
            ;
    */
    fn type_statement(&mut self, visibility: MemberVisibility) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Type)?;
        let mut declarations = vec![self.type_declaration()?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.type_declaration()?);
        }

        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_type_statement(declarations, visibility))
    }

    /*
        TypeDeclaration
            : Identifier ('is' | 'extends' | 'implements' | 'includes') TypeExpression
            ;
    */
    fn type_declaration(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let kind = match self._lookahead {
            Some((Token::Is, _)) => {
                self.eat_token(&Token::Is)?;
                TypeDeclarationKind::Is
            }
            Some((Token::Extends, _)) => {
                self.eat_token(&Token::Extends)?;
                TypeDeclarationKind::Extends
            }
            Some((Token::Implements, _)) => {
                self.eat_token(&Token::Implements)?;
                TypeDeclarationKind::Implements
            }
            Some((Token::Includes, _)) => {
                self.eat_token(&Token::Includes)?;
                TypeDeclarationKind::Includes
            },
            Some((Token::Comma, _)) | Some((Token::ExpressionStatementEnd, _)) => {
                // If we see a comma or the end of the statement, it means this is a continuation of a type declaration list
                return Ok(self._ast_factory.create_type_declaration(name, TypeDeclarationKind::None, None));
            }
            _ => return Err(self.error_unexpected_token("is, implements, includes or extends", &self.lookahead_as_string())),
        };
        let type_expr = match self.type_expression()? {
            Some(typ) => Some(Box::new(typ)),
            None => None,
        };
        Ok(self._ast_factory.create_type_declaration(name, kind, type_expr))
    }

    /*
        ImportPathExpression
            : Identifier ('.' Identifier)*
            ;
    */
    fn import_path_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut segments = vec![self.identifier()?];
        while self.match_lookahead_type(|t| t == &Token::Dot) {
            self.eat_token(&Token::Dot)?;
            segments.push(self.identifier()?);
        }
        Ok(self._ast_factory.create_import_path_expression(segments))
    }

    /*
        ExpressionStatement
            : Expression EXPRESSION_END
            ;
    */
    fn expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let expression = self.expression()?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_expression_statement(expression))
    }

    /*
        BlockStatement
            : Indent OptionalStatementList Dedent
            ;
    */
    fn block_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Indent)?;
        let body = match &self._lookahead {
            Some((Token::Dedent, _)) => vec![], // Empty block
            _ => self.statement_list()?,
        };
        self.eat_token(&Token::Dedent)?;
        Ok(self._ast_factory.create_block(body))
    }

    /*
        Expression
            : AssignmentExpression
            ;
    */
    fn expression(&mut self) -> Result<Expression, SyntaxError> {
        let expression = self.assignment_expression()?;
        Ok(expression)
    }

    /*
        ConditionalExpression
            : LogicalOrExpression
            | LogicalOrExpression 'if' Expression ('else' Expression)
            | LogicalOrExpression 'unless' Expression ('else' Expression)
            ;
    */
    fn conditional_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.logical_or_expression()?;

        let conditional_token = match &self._lookahead {
            Some((Token::If, _)) => Token::If,
            Some((Token::Unless, _)) => Token::Unless,
            _ => return Ok(expression),
        };

        let if_statement_type = if conditional_token == Token::If {
            IfStatementType::If
        } else {
            IfStatementType::Unless
        };

        self.eat_token(&conditional_token)?;
        let condition = self.expression()?;

        self.try_eat_expression_end();

        let else_branch = if self.match_lookahead_type(|t| t == &Token::Else) {
            self.eat_token(&Token::Else)?;
            Some(self.expression()?)
        } else {
            None
        };

        expression = self._ast_factory.create_conditional_expression(expression, condition, else_branch, if_statement_type);
        
        Ok(expression)
    }

    /*
        AssignmentExpression
            : ConditionalExpression
            | LeftHandSideExpression ASSIGNMENT_OPERATOR AssignmentExpression
            ;
    */
    fn assignment_expression(&mut self) -> Result<Expression, SyntaxError> {
        let left = self.conditional_expression()?;

        if !self.lookahead_is_assignment_op() {
            return Ok(left);
        }

        let op = match self.eat_binary_op(is_assignment_op) {
            Ok(token) => match token.0 {
                Token::Assign => AssignmentOp::Assign,
                Token::AssignAdd => AssignmentOp::AssignAdd,
                Token::AssignSub => AssignmentOp::AssignSub,
                Token::AssignMul => AssignmentOp::AssignMul,
                Token::AssignDiv => AssignmentOp::AssignDiv,
                Token::AssignMod => AssignmentOp::AssignMod,
                _ => return Err(
                    self.error_unexpected_operator(token, "=, +=, -=, *=, /=, %=")
                ),
            },
            Err(err) => return Err(err),
        };

        let left = match &left {
            Expression::Identifier(_, class) => {
                if class.is_some() {
                    // A left-hand side identifier cannot be namespaced.
                    return Err(self.error_invalid_left_hand_side_expression());
                }
                self._ast_factory.create_left_hand_side_identifier(left)
            },
            Expression::Member(_, _) => self._ast_factory.create_left_hand_side_member(left),
            Expression::Index(_, _) => self._ast_factory.create_left_hand_side_index(left),
            // Other left-hand side expression types can be added here in the future
            _ => return Err(
                self.error_invalid_left_hand_side_expression()
            ),
        };

        let right = self.assignment_expression()?;

        let assignment_expression = self._ast_factory.create_assignment_expression(
            left,
            op,
            right,
        );

        Ok(assignment_expression)
    }

    /*
        x > y
        x < y
        x >= y
        x <= y

        RelationalExpression
            : AdditiveExpression
            | AdditiveExpression RELATIONAL_OPERATOR RelationalExpression
            ;
    */
    fn relational_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression(
            Self::additive_expression,
            is_relational_op,
            Self::eat_relational_op
        )
    }

    /*
        x == y
        x != y

        EqualityExpression
            : RelationalExpression EQUALITY_OPERATOR EqualityExpression
            | RelationalExpression
            ;
    */
    fn equality_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression(
            Self::relational_expression,
            is_equality_op,
            Self::eat_equality_op
        )
    }

    /*
        x and y
    
        LogicalAndExpression
            : EqualityExpression AND LogicalAndExpression
            | EqualityExpression
            ;
    */
    fn logical_and_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.logical_expression(
            Self::equality_expression,
            is_logical_and_op,
            Self::eat_logical_and_op
        )
    }

    /*
        x or y
    
        LogicalOrExpression
            : LogicalAndExpression OR LogicalOrExpression
            | LogicalOrExpression
            ;
    */
    fn logical_or_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.logical_expression(
            Self::logical_and_expression,
            is_logical_or_op,
            Self::eat_logical_or_op
        )
    }

    /*
        LeftHandSideExpression
            : CallMemberExpression
            ;
    */
    fn left_hand_side_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.call_member_expression()
    }

    /*
        CallMemberExpression
            : PrimaryExpression
            | CallMemberExpression '.' Identifier
            | CallMemberExpression '[' Expression ']'
            | CallMemberExpression Arguments
            ;
    */
    fn call_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.primary_expression()?;

        loop {
            if !self.lookahead_is_member_expression_boundary() {
                break;
            }

            expression = match &self._lookahead {
                Some((Token::Dot, _)) => {
                    self.eat_token(&Token::Dot)?;
                    let property = self.identifier()?;
                    self._ast_factory.create_member_expression(expression, property)
                },
                Some((Token::LBracket, _)) => {
                    self.eat_token(&Token::LBracket)?;
                    let index = self.expression()?;
                    self.eat_token(&Token::RBracket)?;
                    self._ast_factory.create_index_expression(expression, index)
                },
                Some((Token::LParen, _)) => {
                    let args = self.arguments()?;
                    self._ast_factory.create_call_expression(expression, args)
                },
                _ => break,
            };
        }

        Ok(expression)
    }

    /*
        Arguments
            : '(' ')'
            | '(' ArgumentList ')'
    */
    fn arguments(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(&Token::LParen)?;

        let argument_list = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.argument_list()?
        };

        self.eat_token(&Token::RParen)?;
        Ok(argument_list)
    }

    /*
        ArgumentList
            : AssignmentExpression
            | ArgumentList ',' AssignmentExpression
    */
    fn argument_list(&mut self) -> Result<Vec<Expression>, SyntaxError> {
        let mut args = Vec::new();

        args.push(self.assignment_expression()?);

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            args.push(self.assignment_expression()?);
        }

        Ok(args)
    }

    /*
        Identifier
            : IDENTIFIER
            | IDENTIFIER '::' IDENTIFIER
            ;
    */
    fn identifier(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let token = self.eat_token(&Token::Identifier)?;
                let (name, class) = match &self._lookahead {
                    Some((Token::DoubleColon, _)) => {
                        
                        self.eat_token(&Token::DoubleColon)?;
                        let second_token = self.eat_token(&Token::Identifier)?;

                        (self.source[second_token.1.start..second_token.1.end].to_string(), Some(self.source[token.1.start..token.1.end].to_string()))
                    },
                    _ => (self.source[token.1.start..token.1.end].to_string(), None),
                };
                Ok(self._ast_factory.create_identifier_expression(name.to_string(), class))
            },
            _ => Err(
                self.error_unexpected_lookahead_token("identifier")
            ),
        }
    }

    /*
        AdditiveExpression
            : MultiplicativeExpression
            | AdditiveExpression ADDITIVE_OPERATOR MultiplicativeExpression
            ;
    */
    fn additive_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression(
            Self::multiplicative_expression,
            is_additive_op,
            Self::eat_additive_op
        )
    }

    /*
        MultiplicativeExpression
            : UnaryExpression
            | MultiplicativeExpression MULTIPLICATIVE_OPERATOR UnaryExpression
            ;
    */
    fn multiplicative_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression(
            Self::unary_expression,
            is_multiplicative_op,
            Self::eat_multiplicative_op
        )
    }

    fn generic_binary_expression<F, G, E>(&mut self,
            mut create_branch: F,
            op_predicate: fn(&Token) -> bool,
            mut eat_op: G,
            mut create_expression: E
        ) -> Result<Expression, SyntaxError> 
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
        E: FnMut(&mut Self, Expression, BinaryOp, Expression) -> Expression,
    {
        let mut left = create_branch(self)?;

        while self.match_lookahead_type(op_predicate) {
            let op = match eat_op(self) {
                Ok(value) => value,
                Err(value) => return value,
            };

            let right = create_branch(self)?;

            left = create_expression(self, left, op, right);
        }

        Ok(left)
    }

    fn binary_expression<F, G>(&mut self,
            create_branch: F,
            op_predicate: fn(&Token) -> bool,
            eat_op: G
        ) -> Result<Expression, SyntaxError> 
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
    {
        self.generic_binary_expression(
            create_branch,
            op_predicate,
            eat_op,
            |parser, left, op, right| {
                parser._ast_factory.create_binary_expression(left, op, right)
            }
        )
    }

    fn logical_expression<F, G>(&mut self,
            create_branch: F,
            op_predicate: fn(&Token) -> bool,
            eat_op: G
        ) -> Result<Expression, SyntaxError> 
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
    {
        self.generic_binary_expression(
            create_branch,
            op_predicate,
            eat_op,
            |parser, left, op, right| {
                parser._ast_factory.create_logical_expression(left, op, right)
            }
        )
    }

    /*
        UnaryExpression
            : LeftHandSideExpression
            | ADDITIVE_OPERATOR UnaryExpression
            | NOT UnaryExpression
            | AWAIT UnaryExpression
            ;
    */
    fn unary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Plus, _)) => self.create_unary_expression(&Token::Plus, UnaryOp::Plus),
            Some((Token::Minus, _)) => self.create_unary_expression(&Token::Minus, UnaryOp::Negate),
            Some((Token::Not, _)) => self.create_unary_expression(&Token::Not, UnaryOp::Not),
            Some((Token::Tilde, _)) => self.create_unary_expression(&Token::Tilde, UnaryOp::BitwiseNot),
            Some((Token::Decrement, _)) => self.create_unary_expression(&Token::Decrement, UnaryOp::Decrement),
            Some((Token::Increment, _)) => self.create_unary_expression(&Token::Increment, UnaryOp::Increment),
            Some((Token::Await, _)) => self.create_unary_expression(&Token::Await, UnaryOp::Await),
            _ => self.left_hand_side_expression(),
        }
    }

    fn create_unary_expression(&mut self, token: &Token, op: UnaryOp) -> Result<Expression, SyntaxError> {
        self.eat_token(token)?;
        let operand = self.unary_expression()?;
        Ok(self._ast_factory.create_unary_expression(op, operand))
    }

    /*
        PrimaryExpression
            : Literal
            | ParenthesizedExpression
            | Identifier
            ;
    */
    fn primary_expression(&mut self) -> Result<Expression, SyntaxError> {
        if self._lookahead.is_none() {
            return Err(self.error_eof());
        }

        if self.lookahead_is_literal() {
            return self.literal_expression();
        }

        match &self._lookahead {
            Some((Token::LParen, _)) => self.parenthesized_expression(),
            Some((Token::Identifier, _)) => self.identifier(),
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.lambda_expression()
            },
            _ => Err(
                self.error_unexpected_lookahead_token("literal, parenthesized expression, identifier or lambda")
            ),
        }
    }

    /*
        LambdaExpression
            : 'async'? 'gpu'? 'fn' [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END BlockStatement
            | 'async'? 'gpu'? 'fn' [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] ':' ExpressionStatement EXPRESSION_END
            ;
    */
    fn lambda_expression(&mut self) -> Result<Expression, SyntaxError> {
        let statement = self.function_declaration(MemberVisibility::Public)?;
        match statement {
            Statement::FunctionDeclaration(_, generic_types, parameters, return_type, body, properties) => {
                let lambda = self._ast_factory.create_lambda_expression(
                    generic_types,
                    parameters,
                    return_type,
                    *body,
                    properties
                );
                Ok(lambda)
            },
            Statement::Expression(lambda) => {
                // If the function declaration was parsed as an expression, return it directly
                Ok(lambda)
            },
            _ => panic!("BUG: Unexpected statement in lambda expression {:?}", statement),
        }
    }

    /*
        ParenthesizedExpression
            : '(' Expression ')'
            ;
    */
    fn parenthesized_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let expression = self.expression()?;
        self.eat_token(&Token::RParen)?;
        Ok(expression)
    }

    /*
        LiteralExpression
            : Literal
            ;
    */
    fn literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        let literal = self.literal()?;
        Ok(self._ast_factory.create_literal_expression(literal))
    }

    /*
        Literal
            : IntegerLiteral
            : FloatLiteral
            : StringLiteral
            : BooleanLiteral
            : SymbolLiteral
            ;
    */
    fn literal(&mut self) -> Result<Literal, SyntaxError> {
        match &self._lookahead {
            Some((Token::Int, _)) => self.integer_literal(&Token::Int),
            Some((Token::BinaryNumber, _)) => self.integer_literal(&Token::BinaryNumber),
            Some((Token::HexNumber, _)) => self.integer_literal(&Token::HexNumber),
            Some((Token::OctalNumber, _)) => self.integer_literal(&Token::OctalNumber),
            Some((Token::Float, _)) => self.float_literal(),
            Some((Token::True, _)) => self.boolean_literal(&Token::True),
            Some((Token::False, _)) => self.boolean_literal(&Token::False),
            Some((Token::DoubleQuotedString, _)) => self.string_literal(&Token::DoubleQuotedString),
            Some((Token::SingleQuotedString, _)) => self.string_literal(&Token::SingleQuotedString),
            Some((Token::Symbol, _)) => self.symbol_literal(),
            Some((token, span)) => {
                let token_text = &self.source[span.start..span.end];
                Err(
                    self.error_unexpected_token_with_span(
                        "a valid literal", 
                        &format!("{:?} with value '{}'", token, token_text),
                        span.clone()
                    )
                )
            },
            None => Err(self.error_eof()),
        }
    }

    /*
        IntegerLiteral
            : INT
            ;
    */
    fn integer_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                
                // Parse the value based on the token type
                let value = match token_type {
                    Token::Int => str_value.parse::<i128>().map_err(|_| {
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidIntegerLiteral,
                            token.1.start..token.1.end
                        )
                    })?,
                    Token::BinaryNumber => {
                        // Strip "0b" prefix and parse as base 2
                        i128::from_str_radix(&str_value[2..], 2).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidBinaryLiteral,
                                token.1.start..token.1.end
                            )
                        })?
                    },
                    Token::HexNumber => {
                        // Strip "0x" prefix and parse as base 16
                        i128::from_str_radix(&str_value[2..], 16).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidHexLiteral,
                                token.1.start..token.1.end
                            )
                        })?
                    },
                    Token::OctalNumber => {
                        // Strip "0o" prefix and parse as base 8
                        i128::from_str_radix(&str_value[2..], 8).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidOctalLiteral,
                                token.1.start..token.1.end
                            )
                        })?
                    },
                    _ => return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::UnexpectedToken {
                                expected: "integer literal".to_string(),
                                found: format!("{:?}", token_type),
                            },
                            token.1.start..token.1.end
                        )
                    ),
                };
                
                let literal = match value {
                    v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => self._ast_factory.create_i8_literal(v as i8),
                    v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => self._ast_factory.create_i16_literal(v as i16),
                    v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => self._ast_factory.create_i32_literal(v as i32),
                    v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => self._ast_factory.create_i64_literal(v as i64),
                    v if v >= i128::MIN && v <= i128::MAX => self._ast_factory.create_i128_literal(v),
                    _ => return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::IntegerLiteralOverflow,
                            token.1.start..token.1.end
                        )
                    ),
                };
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    /*
        FloatLiteral
            : FLOAT
            ;
    */
    fn float_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::Float) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                let f32_value = str_value.parse::<f32>().map_err(|_| {
                    SyntaxError::new(
                        SyntaxErrorKind::InvalidFloatLiteral,
                        token.1.start..token.1.end
                    )
                })?;
                let uses_exponent = str_value.contains('e') || str_value.contains('E');
                let f32_str = if uses_exponent {
                    // Count digits after the decimal in the significand (before 'e')
                    let significand = str_value
                        .split(|c| c == 'e' || c == 'E')
                        .next()
                        .unwrap_or("");
                    let decimal_digits = significand
                        .split('.')
                        .nth(1)
                        .unwrap_or("")
                        .len();
                    format!("{:.1$e}", f32_value, decimal_digits)
                } else {
                    let part_after_dot_len = str_value.split('.').nth(1).unwrap_or("").len();
                    format!("{:.1$}", f32_value, part_after_dot_len)
                };                

                fn normalize(s: &str) -> String {
                    let s = s.to_lowercase();
                    if let Some((base, exp)) = s.split_once('e') {
                        let base = base.trim_end_matches('0').trim_end_matches('.');
                        let exp = exp.trim_start_matches('+');
                        format!("{}e{}", base, exp)
                    } else {
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    }
                }

                let normalized_input = normalize(str_value);
                let normalized_f32 = normalize(&f32_str);

                // If the f32 representation matches the original string, return as f32
                if normalized_input == normalized_f32 {
                    Ok(self._ast_factory.create_f32_literal(f32_value))
                } else {
                    // Otherwise, parse as f64
                    let f64_value = str_value.parse::<f64>().map_err(|_| {
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidFloatLiteral,
                            token.1.start..token.1.end
                        )
                    })?;
                    Ok(self._ast_factory.create_f64_literal(f64_value))
                }
            },
            Err(e) => Err(e),
        }
    }

    /*
        StringLiteral
            : DoubleQuotedString
            : SingleQuotedString
            ;
    */
    fn string_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end - 1]; // Remove quotes
                let literal = self._ast_factory.create_string_literal(str_value.to_string());
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    /*
        BooleanLiteral
            : TRUE
            : FALSE
            ;
    */
    fn boolean_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end];
                let literal = match str_value {
                    "true" => self._ast_factory.create_boolean_literal(true),
                    "false" => self._ast_factory.create_boolean_literal(false),
                    _ => return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidBooleanLiteral,
                            token.1.start..token.1.end
                        )
                    ),
                };
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    /*
        SymbolLiteral
            : SYMBOL
            ;
    */
    fn symbol_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::Symbol) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end]; // Remove leading colon
                let literal = self._ast_factory.create_symbol_literal(str_value.to_string());
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    fn eat(&mut self, expected: impl Fn(&Token) -> bool, expected_str: &str) -> Result<TokenSpan, SyntaxError> {
        let token = &self._lookahead;

        match token {
            Some((ref t, ref span)) if expected(t) => {
                let result = (t.clone(), span.clone());
                self._lookahead = self.lexer.next().transpose()?;
                Ok(result)
            },
            Some((found, _)) => {
                Err(
                    SyntaxError::new(
                    SyntaxErrorKind::UnexpectedToken {
                        expected: expected_str.to_string(),
                        found: token_to_string(found),
                    },
                    self.source.len()..self.source.len()
                    )
                )
            },
            None => {
                if expected(&Token::ExpressionStatementEnd) {
                    // Special case for end of expression
                    self._lookahead = None;
                    return Ok((Token::ExpressionStatementEnd, 0..0));
                }

                Err(self.error_eof())
            },
        }
    }

    fn eat_token(&mut self, expected: &Token) -> Result<TokenSpan, SyntaxError> {
        self.eat(|t| t == expected, &token_to_string(expected))
    }

    fn eat_binary_op(&mut self, match_token: fn(&Token) -> bool) -> Result<TokenSpan, SyntaxError> {
        self.eat(|t| match_token(t), "binary operator")
    }

    fn match_lookahead_type(&self, match_token: fn(&Token) -> bool) -> bool {
        if let Some((token, _)) = &self._lookahead {
            match_token(token)
        } else {
            false
        }
    }

    fn lookahead_is_assignment_op(&self) -> bool {
        self.match_lookahead_type(is_assignment_op)
    }

    fn lookahead_is_literal(&self) -> bool {
        self.match_lookahead_type(is_literal)
    }

    fn lookahead_is_colon(&self) -> bool {
        self.match_lookahead_type(is_colon)
    }

    fn lookahead_is_comma(&self) -> bool {
        self.match_lookahead_type(is_comma)
    }

    fn lookahead_is_expression_end(&self) -> bool {
        self.match_lookahead_type(is_expression_end)
    }

    fn lookahead_is_else(&self) -> bool {
        self.match_lookahead_type(is_else)
    }

    fn lookahead_is_indent(&self) -> bool {
        self.match_lookahead_type(is_indent)
    }

    fn lookahead_is_dedent(&self) -> bool {
        self.match_lookahead_type(is_dedent)
    }

    fn lookahead_as_string(&self) -> String {
        self._lookahead.as_ref().map_or("end of file".to_string(), |(t, _)| token_to_string(t))
    }

    fn lookahead_is_guard(&self) -> bool {
        self.match_lookahead_type(is_guard)
    }

    fn lookahead_is_in(&self) -> bool {
        self.match_lookahead_type(is_in)
    }

    fn lookahead_is_rparen(&self) -> bool {
        self.match_lookahead_type(is_rparen)
    }

    fn lookahead_is_less_than(&self) -> bool {
        self.match_lookahead_type(is_less_than)
    }

    fn lookahead_is_member_expression_boundary(&self) -> bool {
        self.match_lookahead_type(is_member_expression_boundary)
    }

    fn lookahead_is_inheritance_modifier(&self) -> bool {
        self.match_lookahead_type(is_inheritance_modifier)
    }

    fn lookahead_is_function_modifier(&self) -> bool {
        self.match_lookahead_type(is_function_modifier)
    }

    fn eat_additive_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_additive_op) {
            Ok(token) => match token.0 {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                Token::Pipe => BinaryOp::BitwiseOr,
                Token::Ampersand => BinaryOp::BitwiseAnd,
                Token::Caret => BinaryOp::BitwiseXor,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "+, -, |, &, ^"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_relational_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_relational_op) {
            Ok(token) => match token.0 {
                Token::LessThan => BinaryOp::LessThan,
                Token::LessThanEqual => BinaryOp::LessThanEqual,
                Token::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                Token::GreaterThan => BinaryOp::GreaterThan,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "<, <=, >, >="))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_equality_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_equality_op) {
            Ok(token) => match token.0 {
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "=, !="))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_and_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_and_op) {
            Ok(token) => match token.0 {
                Token::And => BinaryOp::And,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "and"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_or_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_or_op) {
            Ok(token) => match token.0 {
                Token::Or => BinaryOp::Or,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "or"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_multiplicative_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_multiplicative_op) {
            Ok(token) => match token.0 {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "*, /, %"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_expression_end(&mut self) -> Result<TokenSpan, SyntaxError> {
        self.eat_token(&Token::ExpressionStatementEnd)
    }

    fn try_eat_expression_end(&mut self) {
        if self.lookahead_is_expression_end() {
            let _ = self.eat_expression_end();
        }
    }

    fn eat_colon(&mut self) -> Result<TokenSpan, SyntaxError> {
        self.eat_token(&Token::Colon)
    }

    fn error_unexpected_operator(&self, token: TokenSpan, expected: &str) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: self.lookahead_as_string(),
            },
            token.1.start..token.1.end
        )
    }

    fn error_unexpected_token(&self, expected: &str, found: &str) -> SyntaxError {
        self.error_unexpected_token_with_span(expected, found, self.source.len()..self.source.len())
    }

    fn error_unexpected_token_with_span(&self, expected: &str, found: &str, span: Span) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: found.to_string(),
            },
            span
        )
    }

    fn error_unexpected_lookahead_token(&self, expected: &str) -> SyntaxError {
        self.error_unexpected_token(expected, &self.lookahead_as_string())
    }

    fn error_eof(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedEOF,
            self.source.len()..self.source.len()
        )
    }

    fn error_invalid_left_hand_side_expression(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidLeftHandSideExpression,
            self.source.len()..self.source.len()
        )
    }

    fn error_invalid_type_declaration(&self, expected: &str) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidTypeDeclaration { expected: expected.to_string() },
            self.source.len()..self.source.len()
        )
    }

    fn error_missing_struct_member_type(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingStructMemberType,
            self.source.len()..self.source.len()
        )
    }
}

fn is_additive_op(token: &Token) -> bool {
    matches!(token, Token::Plus | Token::Minus | Token::Pipe | Token::Ampersand | Token::Caret)
}

fn is_relational_op(token: &Token) -> bool {
    matches!(token, Token::LessThan | Token::LessThanEqual | Token::GreaterThanEqual | Token::GreaterThan)
}

fn is_equality_op(token: &Token) -> bool {
    matches!(token, Token::Equal | Token::NotEqual)
}

fn is_logical_and_op(token: &Token) -> bool {
    matches!(token, Token::And)
}

fn is_logical_or_op(token: &Token) -> bool {
    matches!(token, Token::Or)
}

fn is_multiplicative_op(token: &Token) -> bool {
    matches!(token, Token::Star | Token::Slash | Token::Percent)
}

fn is_assignment_op(token: &Token) -> bool {
    matches!(token, Token::Assign | Token::AssignAdd | Token::AssignSub | Token::AssignMul | Token::AssignDiv | Token::AssignMod)
}

fn is_literal(token: &Token) -> bool {
    matches!(token, Token::Int | Token::BinaryNumber | Token::HexNumber | Token::OctalNumber | Token::Float | Token::True | Token::False | Token::DoubleQuotedString | Token::SingleQuotedString | Token::Symbol)
}

fn is_colon(token: &Token) -> bool {
    matches!(token, Token::Colon)
}

fn is_comma(token: &Token) -> bool {
    matches!(token, Token::Comma)
}

fn is_expression_end(token: &Token) -> bool {
    matches!(token, Token::ExpressionStatementEnd)
}

fn is_else(token: &Token) -> bool {
    matches!(token, Token::Else)
}

fn is_indent(token: &Token) -> bool {
    matches!(token, Token::Indent)
}

fn is_dedent(token: &Token) -> bool {
    matches!(token, Token::Dedent)
}

fn is_guard(token: &Token) -> bool {
    matches!(token, Token::GreaterThan | Token::GreaterThanEqual | Token::LessThan | Token::LessThanEqual | Token::In | Token::Not | Token::NotEqual)
}

fn is_in(token: &Token) -> bool {
    matches!(token, Token::In)
}

fn is_rparen(token: &Token) -> bool {
    matches!(token, Token::RParen)
}

fn is_less_than(token: &Token) -> bool {
    matches!(token, Token::LessThan)
}

fn is_member_expression_boundary(token: &Token) -> bool {
    matches!(token, Token::LBracket | Token::Dot | Token::LParen)
}

fn is_inheritance_modifier(token: &Token) -> bool {
    matches!(token, Token::Extends | Token::Includes | Token::Implements)
}

fn is_function_modifier(token: &Token) -> bool {
    matches!(token, Token::Async | Token::Gpu)
}
