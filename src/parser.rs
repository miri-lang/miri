// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use crate::ast::*;
use crate::ast_factory as ast;
use crate::lexer::{token_to_string, Lexer, Token, TokenSpan};
use crate::syntax_error::{Span, SyntaxError, SyntaxErrorKind};

struct DeclarationBlockConfig<'a> {
    inline_error: &'a str,
    missing_members_error: SyntaxErrorKind,
}

pub struct Parser<'source> {
    lexer: &'source mut Lexer<'source>,
    source: &'source str,
    _lookahead: Option<TokenSpan>,
}

impl<'source> Parser<'source> {
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str) -> Self {
        Parser {
            lexer,
            source,
            _lookahead: None,
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
        Ok(ast::program(statements))
    }

    /*
        StatementList
            : Statement
            | StatementList Statement
            ;
    */
    fn statement_list(&mut self) -> Result<Vec<Statement>, SyntaxError> {
        let mut statements = vec![];

        // Keep parsing statements until we hit the end of the file or a dedent.
        while self._lookahead.is_some() && !self.lookahead_is_dedent() {
            statements.push(self.statement()?);
            self.try_eat_expression_end();
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
            }
            Some((Token::Protected, _)) => {
                self.eat_token(&Token::Protected)?;
                self.class_member_statement(MemberVisibility::Protected)?
            }
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                self.class_member_statement(MemberVisibility::Private)?
            }
            Some((Token::Indent, _)) => self.block_statement()?,
            Some((Token::Let, _)) | Some((Token::Var, _)) => {
                self.variable_statement(MemberVisibility::Public)?
            }
            Some((Token::If, _)) => self.if_statement(IfStatementType::If)?,
            Some((Token::Unless, _)) => self.if_statement(IfStatementType::Unless)?,
            Some((Token::While, _)) => self.while_statement(WhileStatementType::While)?,
            Some((Token::Until, _)) => self.while_statement(WhileStatementType::Until)?,
            Some((Token::Do, _)) => self.while_statement(WhileStatementType::DoWhile)?,
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
            Some((Token::Extends, _)) => self.extends_statement()?,
            Some((Token::Implements, _)) => self.implements_statement()?,
            Some((Token::Includes, _)) => self.includes_statement()?,
            _ => self.expression_statement()?,
        };
        Ok(statement)
    }

    fn class_member_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let statement = match &self._lookahead {
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement(visibility)?,
            Some((Token::Async, _)) | Some((Token::Fn, _)) | Some((Token::Gpu, _)) => {
                self.function_declaration(visibility)?
            }
            Some((Token::Enum, _)) => self.enum_statement(visibility)?,
            Some((Token::Struct, _)) => self.struct_statement(visibility)?,
            Some((Token::Type, _)) => self.type_statement(visibility)?,
            _ => {
                return Err(self.error_unexpected_lookahead_token(
                    "let, var, async, def, gpu, enum, type or struct",
                ));
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
    fn variable_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let (token, variable_declaration_type) = match &self._lookahead {
            Some((Token::Let, _)) => (Token::Let, VariableDeclarationType::Immutable),
            Some((Token::Var, _)) => (Token::Var, VariableDeclarationType::Mutable),
            _ => Err(self.error_unexpected_lookahead_token("let or var"))?,
        };

        self.eat_token(&token)?;
        let declarations = self.variable_declaration_list(&variable_declaration_type, true)?;
        Ok(ast::variable_statement(declarations, visibility))
    }

    /*
        VariableDeclarationList
            : VariableDeclaration
            | VariableDeclarationList ',' VariableDeclaration
            ;
    */
    fn variable_declaration_list(
        &mut self,
        declaration_type: &VariableDeclarationType,
        accept_initializer: bool,
    ) -> Result<Vec<VariableDeclaration>, SyntaxError> {
        let mut declarations =
            vec![self.variable_declaration(declaration_type, accept_initializer)?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.variable_declaration(declaration_type, accept_initializer)?);
        }

        Ok(declarations)
    }

    fn parse_simple_identifier(&mut self) -> Result<String, SyntaxError> {
        let identifier_expr = self.identifier()?;
        if let ExpressionKind::Identifier(id, class_opt) = identifier_expr.node {
            if let Some(class) = class_opt {
                // A simple identifier cannot be namespaced.
                return Err(self
                    .error_unexpected_token("a simple identifier", &format!("{}::{}", class, id)));
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
    fn variable_declaration(
        &mut self,
        declaration_type: &VariableDeclarationType,
        accept_initializer: bool,
    ) -> Result<VariableDeclaration, SyntaxError> {
        let name = self.parse_simple_identifier()?;

        let typ = self.type_expression()?.map(Box::new);

        let initializer = if accept_initializer {
            match &self._lookahead {
                Some((Token::Assign, _)) => {
                    self.eat_token(&Token::Assign)?;
                    opt_expr(self.expression()?)
                }
                _ => None,
            }
        } else {
            None
        };

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

            if self._lookahead.is_none()
                || self.lookahead_is_expression_end()
                || self.lookahead_is_dedent()
                || self.lookahead_is_else()
            {
                self.try_eat_expression_end();
                return Ok(Statement::Empty);
            }
        } else if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;

            if !self.lookahead_is_indent() {
                return Ok(Statement::Empty);
            }
        } else if self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
            // To support `else if`
            return self.statement();
        } else {
            return Err(self.error_unexpected_lookahead_token("a colon or an expression end"));
        }

        if self._lookahead.is_some() {
            return self.statement();
        }

        Ok(Statement::Empty)
    }

    /*
        IfStatement
            : 'if' Expression ':' ExpressionStatement EXPRESSION_END ('else' ExpressionStatement EXPRESSION_END)?
            | 'if' Expression EXPRESSION_END BlockStatement ('else' EXPRESSION_END BlockStatement)?
            ;
    */
    fn if_statement(
        &mut self,
        if_statement_type: IfStatementType,
    ) -> Result<Statement, SyntaxError> {
        if if_statement_type == IfStatementType::Unless {
            self.eat_token(&Token::Unless)?;
        } else {
            self.eat_token(&Token::If)?;
        }
        let condition = self.expression()?;
        let then_block = self.statement_body()?;

        self.try_eat_expression_end();

        let else_block = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            Some(self.statement_body()?)
        } else {
            None
        };

        if if_statement_type == IfStatementType::Unless {
            Ok(ast::unless_statement(condition, then_block, else_block))
        } else {
            Ok(ast::if_statement(condition, then_block, else_block))
        }
    }

    /*
        WhileStatement
            : 'while' Expression ':' ExpressionStatement EXPRESSION_END
            | 'while' Expression EXPRESSION_END BlockStatement
            | 'until' Expression ':' ExpressionStatement EXPRESSION_END
            | 'until' Expression EXPRESSION_END BlockStatement
            : 'do' ':' ExpressionStatement 'while' Expression EXPRESSION_END
            : 'do' ExpressionStatement 'while' Expression EXPRESSION_END
            | 'forever' ':' ExpressionStatement EXPRESSION_END
            | 'forever' EXPRESSION_END BlockStatement
            ;
    */
    fn while_statement(
        &mut self,
        mut while_statement_type: WhileStatementType,
    ) -> Result<Statement, SyntaxError> {
        let condition;
        let then_block;

        if while_statement_type == WhileStatementType::Until {
            self.eat_token(&Token::Until)?;
            condition = self.expression()?;
            then_block = self.statement_body()?;
        } else if while_statement_type == WhileStatementType::Forever {
            self.eat_token(&Token::Forever)?;
            condition = ast::literal(ast::boolean(true));
            then_block = self.statement_body()?;
        } else if while_statement_type == WhileStatementType::DoWhile {
            self.eat_token(&Token::Do)?;
            then_block = self.statement_body()?;
            match &self._lookahead {
                Some((Token::While, _)) => {
                    self.eat_token(&Token::While)?;
                }
                Some((Token::Until, _)) => {
                    self.eat_token(&Token::Until)?;
                    while_statement_type = WhileStatementType::DoUntil;
                }
                _ => return Err(self.error_unexpected_lookahead_token("while or until")),
            }
            condition = self.expression()?;
        } else {
            self.eat_token(&Token::While)?;
            condition = self.expression()?;
            then_block = self.statement_body()?;
        }

        Ok(ast::while_statement_with_type(
            condition,
            then_block,
            while_statement_type,
        ))
    }

    /*
        RangeExpression
            : LeftHandSideExpression
            | LeftHandSideExpression .. LeftHandSideExpression
            | LeftHandSideExpression ..= LeftHandSideExpression
            ;
    */
    fn range_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.left_hand_side_expression()?;
        let end: Option<Box<Expression>>;
        let range_type;

        match &self._lookahead {
            Some((Token::Range, _)) => {
                self.eat_token(&Token::Range)?;
                range_type = RangeExpressionType::Exclusive;
                end = opt_expr(self.left_hand_side_expression()?);
            }
            Some((Token::RangeInclusive, _)) => {
                self.eat_token(&Token::RangeInclusive)?;
                range_type = RangeExpressionType::Inclusive;
                end = opt_expr(self.left_hand_side_expression()?);
            }
            _ => {
                range_type = RangeExpressionType::IterableObject;
                end = None;
            }
        };

        Ok(ast::range(start, end, range_type))
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
        let variable_declarations =
            self.variable_declaration_list(&VariableDeclarationType::Immutable, false)?;
        self.eat_token(&Token::In)?;
        let iterable = self.range_expression()?;

        if let ExpressionKind::Range(_, _, range_type) = &iterable.node {
            if *range_type != RangeExpressionType::IterableObject && variable_declarations.len() > 1
            {
                return Err(self.error_unexpected_token(
                    "a single loop variable for a numeric range",
                    &format!("{} variables", variable_declarations.len()),
                ));
            }
        }

        let body = self.statement_body()?;

        Ok(ast::for_statement(variable_declarations, iterable, body))
    }

    /*
        FunctionDeclaration
            : 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END BlockStatement
            | 'async'? 'gpu'? 'fn' Identifier [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] ':' ExpressionStatement EXPRESSION_END
            ;
    */
    fn function_declaration(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
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
                }
                Some((Token::Gpu, _)) => {
                    self.eat_token(&Token::Gpu)?;
                    properties.is_gpu = true;
                }
                _ => {
                    return Err(
                        self.error_unexpected_lookahead_token("function modifier (async or gpu)")
                    )
                }
            }
        }

        self.eat_token(&Token::Fn)?;

        let name = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let token = self.eat_token(&Token::Identifier)?;
                self.source[token.1.start..token.1.end].to_string()
            }
            Some((Token::LessThan, _)) | Some((Token::LParen, _)) => {
                // No name, it's a lambda
                "".to_string()
            }
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
                ast::expression_statement(expr)
            } else {
                // A block lambda body is a normal block statement, which statement_body handles correctly.
                self.statement_body()?
            }
        } else {
            // This is a named function. Its body is always a full statement.
            self.statement_body()?
        };

        if name.is_empty() {
            return Ok(ast::expression_statement(ast::lambda_expression(
                generic_types,
                parameters,
                return_type,
                body,
                properties,
            )));
        }

        Ok(ast::function_declaration(
            &name,
            generic_types,
            parameters,
            return_type,
            body,
            properties,
        ))
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
        let parameters = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.parameter_list()?
        };
        self.eat_token(&Token::RParen)?;

        Ok(parameters)
    }

    fn return_type_expression(&mut self) -> Result<Option<Box<Expression>>, SyntaxError> {
        let return_type = self.type_expression()?.map(Box::new);
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
            return Ok(ast::generic_type_expression(
                identifier,
                None,
                TypeDeclarationKind::None,
            ));
        }

        let token_span = self.eat(is_inheritance_modifier, "extends, includes or implements")?;
        let kind = match token_span.0 {
            Token::Extends => TypeDeclarationKind::Extends,
            Token::Implements => TypeDeclarationKind::Implements,
            Token::Includes => TypeDeclarationKind::Includes,
            _ => TypeDeclarationKind::None,
        };

        let typ = self.type_expression()?.map(Box::new);

        Ok(ast::generic_type_expression(identifier, typ, kind))
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
            // Allow an optional trailing comma before the closing parenthesis.
            if self.lookahead_is_rparen() {
                break;
            }
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
            Some(typ) => Box::new(typ),
            None => {
                // Miri doesn't support untyped parameters
                return Err(self.error_missing_type_expression());
            }
        };

        let guard = if self._lookahead.is_some() && self.lookahead_is_guard() {
            opt_expr(self.guard_expression()?)
        } else {
            None
        };

        let default_value = if self.match_lookahead_type(|t| t == &Token::Assign) {
            self.eat_token(&Token::Assign)?;
            Some(Box::new(self.expression()?))
        } else {
            None
        };

        Ok(Parameter {
            name,
            typ,
            guard,
            default_value,
        })
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
        Ok(ast::guard(guard_op, expression))
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
        self.eat_statement_end()?;
        Ok(ast::return_statement(expression))
    }

    /*
        BreakStatement
            : 'break' EXPRESSION_END
            ;
    */
    fn break_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Break)?;
        self.eat_statement_end()?;
        Ok(ast::break_statement())
    }

    /*
        ContinueStatement
            : 'continue' EXPRESSION_END
            ;
    */
    fn continue_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Continue)?;
        self.eat_statement_end()?;
        Ok(ast::continue_statement())
    }

    fn parse_declaration_block<F, C>(
        &mut self,
        item_parser: F,
        creator: C,
        name: Expression,
        visibility: MemberVisibility,
        config: DeclarationBlockConfig,
        generic_types: Option<Vec<Expression>>,
    ) -> Result<Statement, SyntaxError>
    where
        F: Fn(&mut Self) -> Result<Expression, SyntaxError>,
        C: Fn(Expression, Option<Vec<Expression>>, Vec<Expression>, MemberVisibility) -> Statement,
    {
        let mut items = vec![];

        match &self._lookahead {
            Some((Token::Colon, _)) => {
                // Inline form
                self.eat_token(&Token::Colon)?;
                if !self.lookahead_is_expression_end() && self._lookahead.is_some() {
                    items.push(item_parser(self)?);
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        items.push(item_parser(self)?);
                    }
                }
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                // Block form
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.eat_token(&Token::Indent)?;
                    while !self.lookahead_is_dedent() {
                        items.push(item_parser(self)?);
                        self.try_eat_expression_end();
                    }
                    self.eat_token(&Token::Dedent)?;
                }
            }
            _ => return Err(self.error_unexpected_lookahead_token(config.inline_error)),
        };

        if items.is_empty() {
            return Err(self.error_missing_members(config.missing_members_error));
        }

        Ok(creator(name, generic_types, items, visibility))
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
            |n, _, vals: Vec<Expression>, vis| ast::enum_statement(n, vals, vis),
            name,
            visibility,
            DeclarationBlockConfig {
                inline_error: "either a colon for inline enums or an indentation for block enums",
                missing_members_error: SyntaxErrorKind::MissingEnumMembers,
            },
            None,
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
            self.multiple_element_type_expressions(
                "Enum value type",
                &Token::LParen,
                &Token::RParen,
            )?
        } else {
            vec![]
        };

        Ok(ast::enum_value_expression(identifier, types))
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
        let generic_types = self.generic_types_expression()?;
        self.parse_declaration_block(
            Self::struct_member_expression,
            ast::struct_statement,
            name,
            visibility,
            DeclarationBlockConfig {
                inline_error:
                    "either a colon for inline structs or an indentation for block structs",
                missing_members_error: SyntaxErrorKind::MissingStructMembers,
            },
            generic_types,
        )
    }

    /*
        StructMember
            : Identifier TypeExpression
            ;
    */
    fn struct_member_expression(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let typ = self
            .type_expression()?
            .ok_or_else(|| self.error_missing_struct_member_type())?;
        Ok(ast::struct_member_expression(name, typ))
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

        let base_typ_expr: Option<Expression> = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let type_name = self.identifier_to_type_name()?;
                let typ = self.type_name_to_type(type_name)?;
                Some(ast::typ(typ))
            }
            Some((Token::LBracket, _)) => {
                self.eat_token(&Token::LBracket)?;
                let element_type = self.element_type_expression("List element type")?;
                self.eat_token(&Token::RBracket)?;
                Some(ast::typ(Type::List(Box::new(element_type))))
            }
            Some((Token::LParen, _)) => {
                self.eat_token(&Token::LParen)?;
                if self.lookahead_is_rparen() {
                    self.eat_token(&Token::RParen)?;
                    // Empty tuple type `()`
                    return Ok(Some(ast::typ(Type::Tuple(vec![]))));
                }

                let first_element =
                    self.element_type_expression("Grouped type or tuple element")?;

                if self.lookahead_is_comma() {
                    let mut elements = vec![first_element];
                    while self.lookahead_is_comma() {
                        self.eat_token(&Token::Comma)?;
                        if self.lookahead_is_rparen() {
                            break;
                        } // Allow trailing comma
                        elements.push(self.element_type_expression("Tuple element type")?);
                    }
                    self.eat_token(&Token::RParen)?;
                    Some(ast::typ(Type::Tuple(elements)))
                } else {
                    self.eat_token(&Token::RParen)?;
                    Some(first_element)
                }
            }
            Some((Token::LBrace, _)) => {
                self.eat_token(&Token::LBrace)?;
                let key_type = self.element_type_expression("Map key type")?;
                let typ = if self.match_lookahead_type(|t| t == &Token::Colon) {
                    self.eat_token(&Token::Colon)?;
                    let value_type = self.element_type_expression("Map value type")?;
                    self.eat_token(&Token::RBrace)?;
                    Type::Map(Box::new(key_type), Box::new(value_type))
                } else {
                    self.eat_token(&Token::RBrace)?;
                    Type::Set(Box::new(key_type))
                };
                Some(ast::typ(typ))
            }
            Some((Token::Fn, _)) => {
                self.eat_token(&Token::Fn)?;
                let generic_types = self.generic_types_expression()?;

                self.eat_token(&Token::LParen)?;
                let mut parameters = Vec::new();
                if !self.lookahead_is_rparen() {
                    loop {
                        if self.lookahead_is_rparen() {
                            break;
                        }

                        // Parse first type expression
                        let first_type_expr = if let Some(typ) = self.type_expression()? {
                            typ
                        } else {
                            return Err(self.error_missing_type_expression());
                        };

                        // Check what follows to decide if first_type_expr is a name or a type
                        let is_named_param =
                            if self.lookahead_is_comma() || self.lookahead_is_rparen() {
                                false
                            } else {
                                // If it's not comma or rparen, it must be the start of another type expression
                                // Check if the next token can start a type expression
                                self.match_lookahead_type(|t| {
                                    matches!(
                                        t,
                                        Token::Identifier
                                            | Token::LBracket
                                            | Token::LParen
                                            | Token::LBrace
                                            | Token::Fn
                                    )
                                })
                            };

                        if is_named_param {
                            // The first expression was the name.
                            let param_name = if let ExpressionKind::Type(ty, is_nullable) =
                                &first_type_expr.node
                            {
                                if *is_nullable {
                                    return Err(self.error_unexpected_token(
                                        "Parameter name cannot be nullable",
                                        "identifier",
                                    ));
                                }
                                match &**ty {
                                    Type::Custom(name, None) => name.clone(),
                                    _ => {
                                        return Err(self.error_unexpected_token(
                                            "Parameter name must be a simple identifier",
                                            "identifier",
                                        ))
                                    }
                                }
                            } else {
                                return Err(self.error_unexpected_token(
                                    "Expected parameter name",
                                    "identifier",
                                ));
                            };

                            // Now parse the actual type
                            let param_type = if let Some(typ) = self.type_expression()? {
                                typ
                            } else {
                                return Err(self.error_missing_type_expression());
                            };

                            parameters.push(Parameter {
                                name: param_name,
                                typ: Box::new(param_type),
                                guard: None,
                                default_value: None,
                            });
                        } else {
                            // Unnamed parameter
                            parameters.push(Parameter {
                                name: "".to_string(),
                                typ: Box::new(first_type_expr),
                                guard: None,
                                default_value: None,
                            });
                        }

                        if self.lookahead_is_comma() {
                            self.eat_token(&Token::Comma)?;
                            // Allow trailing comma
                            if self.lookahead_is_rparen() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.eat_token(&Token::RParen)?;

                let return_type = self.return_type_expression()?;
                let typ = Type::Function(generic_types, parameters, return_type);
                Some(ast::typ(typ))
            }
            _ => return Ok(None),
        };

        let mut final_expr = match base_typ_expr {
            Some(expr) => expr,
            None => return Ok(None),
        };

        if self.match_lookahead_type(|t| t == &Token::QuestionMark) {
            self.eat_token(&Token::QuestionMark)?;
            if let ExpressionKind::Type(inner_type, _) = final_expr.node {
                final_expr = ast::null_typ(*inner_type);
            }
        }

        Ok(Some(final_expr))
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
            "result" => self.generic_two_types_expression(
                "Ok result type",
                "Error result type",
                Type::Result,
            )?,
            "map" => {
                self.generic_two_types_expression("Map key type", "Map value type", Type::Map)?
            }
            "future" => self.generic_one_type_expression("Future result type", Type::Future)?,
            "list" => self.generic_one_type_expression("List element type", Type::List)?,
            "set" => self.generic_one_type_expression("Set element type", Type::Set)?,
            "tuple" => {
                let inner = self.multiple_element_type_expressions(
                    "Tuple item type",
                    &Token::LessThan,
                    &Token::GreaterThan,
                )?;
                Type::Tuple(inner)
            }
            "fn" => {
                // fn<T>(int) int
                let generic_types = self.generic_types_expression()?;

                // Parse parameter types, not full parameters
                self.eat_token(&Token::LParen)?;
                let mut parameters = Vec::new();
                if !self.lookahead_is_rparen() {
                    loop {
                        if self.lookahead_is_rparen() {
                            break;
                        }

                        // In function type, we only have types, not names
                        // But wait, the AST for Function type uses `Vec<Parameter>`?
                        // Let's check `ast.rs`.
                        // Type::Function(Option<Vec<Expression>>, Vec<Parameter>, Option<Box<Expression>>)
                        // It uses `Vec<Parameter>`. This implies named parameters in function types?
                        // Or maybe just types wrapped in Parameter struct with empty names?
                        // If the user writes `fn(int, string)`, there are no names.
                        // If the user writes `fn(x int, y string)`, there are names.
                        // The parser test `test_function_type_as_return_type` uses `fn() int`.
                        // The failing test `test_lambda_as_argument` uses `fn(int) int`.
                        // So it seems we support unnamed parameters in function types.

                        // Let's try to parse a type expression first.
                        if let Some(typ) = self.type_expression()? {
                            // It's a type. Create a dummy parameter.
                            parameters.push(Parameter {
                                name: "".to_string(),
                                typ: Box::new(typ),
                                guard: None,
                                default_value: None,
                            });
                        } else {
                            return Err(self.error_missing_type_expression());
                        }

                        if self.lookahead_is_comma() {
                            self.eat_token(&Token::Comma)?;
                            // Allow trailing comma
                            if self.lookahead_is_rparen() {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.eat_token(&Token::RParen)?;

                let return_type = self.return_type_expression()?;
                Type::Function(generic_types, parameters, return_type)
            }
            _ => match &self._lookahead {
                Some((Token::LessThan, _)) => {
                    let inner = self.multiple_element_type_expressions(
                        "Generic type",
                        &Token::LessThan,
                        &Token::GreaterThan,
                    )?;
                    Type::Custom(type_name, Some(inner))
                }
                _ => Type::Custom(type_name, None),
            },
        })
    }

    fn identifier_to_type_name(&mut self) -> Result<String, SyntaxError> {
        Ok(match self.identifier()?.node {
            ExpressionKind::Identifier(id, Some(class)) => format!("{}::{}", class, id), // Reconstruct the full path
            ExpressionKind::Identifier(id, None) => id,
            _ => return Err(self.error_unexpected_token("identifier", &self.lookahead_as_string())),
        })
    }

    fn element_type_expression(&mut self, expected: &str) -> Result<Expression, SyntaxError> {
        let element_type = match self.type_expression()? {
            Some(typ) => typ,
            None => return Err(self.error_invalid_type_declaration(expected)),
        };
        Ok(element_type)
    }

    fn multiple_element_type_expressions(
        &mut self,
        expected: &str,
        left_token: &Token,
        right_token: &Token,
    ) -> Result<Vec<Expression>, SyntaxError> {
        self.eat_token(left_token)?;

        let element_type = self.element_type_expression(expected)?;
        let mut elements = vec![element_type];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            match &self._lookahead {
                Some((t, _)) if t == right_token => break, // Allow trailing comma
                None => break,
                _ => {
                    let element_type = self.element_type_expression(expected)?;
                    elements.push(element_type);
                }
            }
        }

        self.eat_token(right_token)?;

        Ok(elements)
    }

    fn generic_one_type_expression<F>(
        &mut self,
        expected: &str,
        create_type: F,
    ) -> Result<Type, SyntaxError>
    where
        F: FnOnce(Box<Expression>) -> Type,
    {
        self.eat_token(&Token::LessThan)?;
        let inner_type = self.element_type_expression(expected)?;
        self.eat_token(&Token::GreaterThan)?;
        Ok(create_type(Box::new(inner_type)))
    }

    fn generic_two_types_expression<F>(
        &mut self,
        expected_a: &str,
        expected_b: &str,
        create_type: F,
    ) -> Result<Type, SyntaxError>
    where
        F: FnOnce(Box<Expression>, Box<Expression>) -> Type,
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
        Ok(ast::use_statement(import_path, alias))
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
            if self.lookahead_is_expression_end() {
                break; // Allow trailing comma
            }
            declarations.push(self.type_declaration()?);
        }
        self.eat_statement_end()?;
        Ok(ast::type_statement(declarations, visibility))
    }

    fn inheritance_identifier(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        match name.node {
            ExpressionKind::Identifier(_, Some(_)) => {
                Err(self.error_invalid_inheritance_identifier())
            }
            ExpressionKind::Identifier(_, None) => Ok(name),
            _ => Err(self.error_unexpected_token("identifier", format!("{:?}", name).as_str())),
        }
    }

    /*
        ExtendsStatement
            : 'extends' Identifier
            ;
    */
    fn extends_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Extends)?;
        let base = self.inheritance_identifier()?;
        self.eat_statement_end()?;
        Ok(ast::extends(base))
    }

    /*
        ImplementsStatement
            : 'implements' Identifier (',' Identifier)*
            ;
    */
    fn implements_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Implements)?;
        let mut trait_names = vec![self.inheritance_identifier()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            trait_names.push(self.inheritance_identifier()?);
        }
        self.eat_statement_end()?;
        Ok(ast::implements(trait_names))
    }

    /*
        IncludesStatement
            : 'includes' Identifier (',' Identifier)*
            ;
    */
    fn includes_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Includes)?;
        let mut module_names = vec![self.inheritance_identifier()?];
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            module_names.push(self.inheritance_identifier()?);
        }
        self.eat_statement_end()?;
        Ok(ast::includes(module_names))
    }

    /*
        TypeDeclaration
            : Identifier ('is' | 'extends' | 'implements' | 'includes') TypeExpression
            ;
    */
    fn type_declaration(&mut self) -> Result<Expression, SyntaxError> {
        let name = self.identifier()?;
        let generic_types = self.generic_types_expression()?;
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
            }
            Some((Token::Comma, _)) | Some((Token::ExpressionStatementEnd, _)) => {
                // If we see a comma or the end of the statement, it means this is a continuation of a type declaration list
                return Ok(ast::type_declaration_expression(
                    name,
                    generic_types,
                    TypeDeclarationKind::None,
                    None,
                ));
            }
            _ => {
                return Err(self.error_unexpected_token(
                    "is, implements, includes or extends",
                    &self.lookahead_as_string(),
                ))
            }
        };
        let type_expr = self.type_expression()?.map(Box::new);
        Ok(ast::type_declaration_expression(
            name,
            generic_types,
            kind,
            type_expr,
        ))
    }

    /*
        ImportPathExpression
            : Identifier ('.' Identifier)* ('.' ('*' | '{' ImportList '}'))?
            ;
    */
    fn import_path_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut segments = vec![self.identifier()?];
        let mut kind = ImportPathKind::Simple;

        while self.match_lookahead_type(|t| t == &Token::Dot) {
            self.eat_token(&Token::Dot)?;

            if self.match_lookahead_type(|t| t == &Token::Star) {
                self.eat_token(&Token::Star)?;
                kind = ImportPathKind::Wildcard;
                break; // Wildcard must be the last part of the path
            }

            if self.match_lookahead_type(|t| t == &Token::LBrace) {
                self.eat_token(&Token::LBrace)?;
                let mut multi_imports = vec![self.multi_import_segment()?];
                while self.lookahead_is_comma() {
                    self.eat_token(&Token::Comma)?;
                    if self.match_lookahead_type(|t| t == &Token::RBrace) {
                        break; // Allow trailing comma
                    }
                    multi_imports.push(self.multi_import_segment()?);
                }
                self.eat_token(&Token::RBrace)?;
                kind = ImportPathKind::Multi(multi_imports);
                break; // Multi-import block must be the last part
            }

            segments.push(self.identifier()?);
        }
        Ok(ast::import_path_expression(segments, kind))
    }

    fn multi_import_segment(
        &mut self,
    ) -> Result<(Expression, Option<Box<Expression>>), SyntaxError> {
        let path = self.identifier()?;
        let alias = if self.match_lookahead_type(|t| t == &Token::As) {
            self.eat_token(&Token::As)?;
            Some(Box::new(self.identifier()?))
        } else {
            None
        };
        Ok((path, alias))
    }

    /*
        ExpressionStatement
            : Expression EXPRESSION_END
            ;
    */
    fn expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let expression = self.expression()?;
        self.eat_statement_end()?;
        Ok(ast::expression_statement(expression))
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
        Ok(ast::block(body))
    }

    /*
        Expression
            : AssignmentExpression
            ;
    */
    fn expression(&mut self) -> Result<Expression, SyntaxError> {
        self.assignment_expression()
    }

    /*
        ConditionalExpression
            : LogicalOrExpression
            | LogicalOrExpression 'if' Expression ('else' Expression)
            | LogicalOrExpression 'unless' Expression ('else' Expression)
            ;
    */
    fn conditional_expression(&mut self) -> Result<Expression, SyntaxError> {
        let expression = self.logical_or_expression()?;

        if !self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
            return Ok(expression);
        }

        let if_statement_type = if self.match_lookahead_type(|t| t == &Token::If) {
            self.eat_token(&Token::If)?;
            IfStatementType::If
        } else {
            self.eat_token(&Token::Unless)?;
            IfStatementType::Unless
        };

        // The condition is also a full expression, which will be parsed with its own precedence.
        let condition = self.conditional_expression()?;

        // The `else` part is optional for a postfix modifier `if`.
        let else_branch = if self.match_lookahead_type(|t| t == &Token::Else) {
            self.eat_token(&Token::Else)?;
            Some(self.conditional_expression()?)
        } else {
            None
        };

        let span = expression.span.start..(if let Some(ref e) = else_branch {
            e.span.end
        } else {
            condition.span.end
        });
        let expression =
            ast::conditional_with_span(expression, condition, else_branch, if_statement_type, span);

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
                _ => return Err(self.error_unexpected_operator(token, "=, +=, -=, *=, /=, %=")),
            },
            Err(err) => return Err(err),
        };

        let left = match &left.node {
            ExpressionKind::Identifier(_, class) => {
                if class.is_some() {
                    // A left-hand side identifier cannot be namespaced.
                    return Err(self.error_invalid_left_hand_side_expression());
                }
                ast::lhs_identifier_from_expr(left)
            }
            ExpressionKind::Member(_, _) => ast::lhs_member_from_expr(left),
            ExpressionKind::Index(_, _) => ast::lhs_index_from_expr(left),
            // Other left-hand side expression types can be added here in the future
            _ => return Err(self.error_invalid_left_hand_side_expression()),
        };

        let right = self.assignment_expression()?;

        let span = left.span().start..right.span.end;
        let assignment_expression = ast::assign_with_span(left, op, right, span);

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
        self._binary_expression(
            Self::additive_expression,
            is_relational_op,
            Self::eat_relational_op,
            ast::binary_with_span,
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
        self._binary_expression(
            Self::relational_expression,
            is_equality_op,
            Self::eat_equality_op,
            ast::binary_with_span,
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
        self._binary_expression(
            Self::equality_expression,
            is_logical_and_op,
            Self::eat_logical_and_op,
            ast::logical_with_span,
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
        self._binary_expression(
            Self::logical_and_expression,
            is_logical_or_op,
            Self::eat_logical_or_op,
            ast::logical_with_span,
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
            | CallMemberExpression '(' Arguments ')'
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
                    let span = expression.span.start..property.span.end;
                    ast::member_with_span(expression, property, span)
                }
                Some((Token::LBracket, _)) => {
                    self.eat_token(&Token::LBracket)?;
                    let index = self.expression()?;
                    let (_, rbracket_span) = self.eat_token(&Token::RBracket)?;
                    let span = expression.span.start..rbracket_span.end;
                    ast::index_with_span(expression, index, span)
                }
                Some((Token::LParen, _)) => {
                    let (args, rparen_span) = self.arguments()?;
                    let span = expression.span.start..rparen_span.end;
                    ast::call_with_span(expression, args, span)
                }
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
    fn arguments(&mut self) -> Result<(Vec<Expression>, Span), SyntaxError> {
        self.eat_token(&Token::LParen)?;

        let argument_list = if self.lookahead_is_rparen() {
            vec![]
        } else {
            self.argument_list()?
        };

        let (_, span) = self.eat_token(&Token::RParen)?;
        Ok((argument_list, span))
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
            // Allow an optional trailing comma before the closing parenthesis.
            if self.lookahead_is_rparen() {
                break;
            }
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
                let (_, span) = self.eat_token(&Token::Identifier)?;
                let (name, class, full_span) = match &self._lookahead {
                    Some((Token::DoubleColon, _)) => {
                        self.eat_token(&Token::DoubleColon)?;
                        let (_, second_span) = self.eat_token(&Token::Identifier)?;

                        (
                            self.source[second_span.start..second_span.end].to_string(),
                            Some(self.source[span.start..span.end].to_string()),
                            span.start..second_span.end,
                        )
                    }
                    _ => (self.source[span.start..span.end].to_string(), None, span),
                };
                Ok(ast::identifier_with_class_and_span(&name, class, full_span))
            }
            _ => Err(self.error_unexpected_lookahead_token("identifier")),
        }
    }

    /*
        AdditiveExpression
            : MultiplicativeExpression
            | AdditiveExpression ADDITIVE_OPERATOR MultiplicativeExpression
            ;
    */
    fn additive_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::multiplicative_expression,
            is_additive_op,
            Self::eat_additive_op,
            ast::binary_with_span,
        )
    }

    /*
        MultiplicativeExpression
            : UnaryExpression
            | MultiplicativeExpression MULTIPLICATIVE_OPERATOR UnaryExpression
            ;
    */
    fn multiplicative_expression(&mut self) -> Result<Expression, SyntaxError> {
        self._binary_expression(
            Self::unary_expression,
            is_multiplicative_op,
            Self::eat_multiplicative_op,
            ast::binary_with_span,
        )
    }

    fn _binary_expression<F, G, E>(
        &mut self,
        mut create_branch: F,
        op_predicate: fn(&Token) -> bool,
        mut eat_op: G,
        mut create_expression: E,
    ) -> Result<Expression, SyntaxError>
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
        E: FnMut(Expression, BinaryOp, Expression, Span) -> Expression,
    {
        let mut left = create_branch(self)?;

        while self.match_lookahead_type(op_predicate) {
            let op = match eat_op(self) {
                Ok(value) => value,
                Err(value) => return value,
            };

            let right = create_branch(self)?;

            let span = left.span.start..right.span.end;
            left = create_expression(left, op, right, span);
        }

        Ok(left)
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
            Some((Token::Tilde, _)) => {
                self.create_unary_expression(&Token::Tilde, UnaryOp::BitwiseNot)
            }
            Some((Token::Decrement, _)) => {
                self.create_unary_expression(&Token::Decrement, UnaryOp::Decrement)
            }
            Some((Token::Increment, _)) => {
                self.create_unary_expression(&Token::Increment, UnaryOp::Increment)
            }
            Some((Token::Await, _)) => self.create_unary_expression(&Token::Await, UnaryOp::Await),
            _ => self.left_hand_side_expression(),
        }
    }

    fn create_unary_expression(
        &mut self,
        token: &Token,
        op: UnaryOp,
    ) -> Result<Expression, SyntaxError> {
        let (_, span) = self.eat_token(token)?;
        let operand = self.unary_expression()?;
        let full_span = span.start..operand.span.end;
        Ok(ast::unary_with_span(op, operand, full_span))
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
            }
            Some((Token::LBracket, _)) => self.list_literal_expression(),
            Some((Token::LBrace, _)) => self.brace_expression(),
            Some((Token::Match, _)) => self.match_expression(),
            Some((Token::FormattedStringStart(_), _)) => self.formatted_string_expression(),
            _ => Err(self.error_unexpected_lookahead_token("an expression")),
        }
    }

    /*
        FormattedStringExpression
            : FormattedStringStart Expression (FormattedStringMiddle Expression)* FormattedStringEnd
            ;
    */
    fn formatted_string_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut parts = Vec::new();

        let start_token_str = &token_to_string(&Token::FormattedStringStart("".to_string()));
        if let Some((Token::FormattedStringStart(start_text), _)) = self._lookahead.clone() {
            self.eat(
                |t| matches!(t, Token::FormattedStringStart(_)),
                start_token_str,
            )?;
            if !start_text.is_empty() {
                parts.push(ast::literal(ast::string_literal(&start_text)));
            }
        } else {
            return Err(self.error_unexpected_lookahead_token(start_token_str));
        }

        while self._lookahead.is_some() {
            parts.push(self.expression()?);

            if let Some((Token::FormattedStringMiddle(middle_text), _)) = self._lookahead.clone() {
                self.eat(
                    |t| matches!(t, Token::FormattedStringMiddle(_)),
                    &token_to_string(&Token::FormattedStringMiddle("".to_string())),
                )?;
                if !middle_text.is_empty() {
                    parts.push(ast::literal(ast::string_literal(&middle_text)));
                }
            } else if let Some((Token::FormattedStringEnd(end_text), _)) = self._lookahead.clone() {
                self.eat(
                    |t| matches!(t, Token::FormattedStringEnd(_)),
                    &token_to_string(&Token::FormattedStringEnd("".to_string())),
                )?;
                if !end_text.is_empty() {
                    parts.push(ast::literal(ast::string_literal(&end_text)));
                }
                break; // End of the f-string
            } else {
                return Err(
                    self.error_unexpected_lookahead_token("middle or end of a formatted string")
                );
            }
        }

        Ok(ast::f_string(parts))
    }

    /*
        MatchExpression
            : 'match' Expression ':' MatchBranchList
            | 'match' Expression INDENT MatchBranchList DEDENT
            ;
    */
    fn match_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::Match)?;
        let value = self.expression()?;
        let mut branches = Vec::new();

        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;
            if self._lookahead.is_some() {
                branches.extend(self.match_branch_list(true)?);
            }
        } else if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;
            if self.lookahead_is_indent() {
                self.eat_token(&Token::Indent)?;
                branches.extend(self.match_branch_list(false)?);
                self.eat_token(&Token::Dedent)?;
            }
        } else {
            return Err(self.error_unexpected_lookahead_token(
                "':' for an inline match or a new line for a block match",
            ));
        }

        if branches.is_empty() {
            return Err(self.error_missing_match_branches());
        }

        // Check for duplicate (pattern, guard) combinations.
        // This catches simple duplicates like `1: ... 1: ...` or
        // `x if x > 10: ... x if x > 10: ...`.
        // A more complex semantic analysis for overlapping or unreachable
        // patterns is left to a later compiler stage.
        let mut seen_pattern_guards = std::collections::HashSet::new();
        for branch in &branches {
            for pattern in &branch.patterns {
                let key = (pattern.clone(), branch.guard.clone());
                if !seen_pattern_guards.insert(key) {
                    // This exact pattern and guard combination has been seen before.
                    return Err(self.error_duplicate_match_pattern());
                }
            }
        }

        Ok(ast::match_expression(value, branches))
    }

    /*
        MatchBranchList
            : MatchBranch (','? MatchBranch)*
            ;
    */
    fn match_branch_list(&mut self, inline_mode: bool) -> Result<Vec<MatchBranch>, SyntaxError> {
        let mut branches = vec![self.match_branch()?];

        while (inline_mode && self.lookahead_is_comma())
            || (!inline_mode && self._lookahead.is_some() && !self.lookahead_is_dedent())
        {
            if inline_mode {
                self.eat_token(&Token::Comma)?;
            }
            branches.push(self.match_branch()?);
        }

        Ok(branches)
    }

    /*
        MatchBranch
            : Pattern ('|' Pattern)* ('if' Expression)? (':' Expression | INDENT StatementList DEDENT) EXPRESSION_END
            ;
    */
    fn match_branch(&mut self) -> Result<MatchBranch, SyntaxError> {
        let mut patterns = vec![self.pattern()?];
        while self.match_lookahead_type(|t| t == &Token::Pipe) {
            self.eat_token(&Token::Pipe)?;
            patterns.push(self.pattern()?);
        }

        let guard = if self.match_lookahead_type(|t| t == &Token::If) {
            self.eat_token(&Token::If)?;
            Some(Box::new(self.expression()?))
        } else {
            None
        };

        let body_parsing_error = self.error_unexpected_lookahead_token(
            "a colon for an inline body or an indented block for a block body",
        );
        let body = match &self._lookahead {
            Some((Token::Colon, _)) => {
                self.eat_token(&Token::Colon)?;
                let expr = self.expression()?;
                ast::expression_statement(expr)
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.block_statement()?
                } else {
                    return Err(body_parsing_error);
                }
            }
            _ => return Err(body_parsing_error),
        };
        self.try_eat_expression_end();

        Ok(MatchBranch {
            patterns,
            guard,
            body: Box::new(body),
        })
    }

    /*
        Pattern
            : Literal
            | Identifier
            | TuplePattern
            | 'default'
            ;
    */
    fn pattern(&mut self) -> Result<Pattern, SyntaxError> {
        match &self._lookahead {
            Some((Token::Default, _)) => {
                self.eat_token(&Token::Default)?;
                Ok(Pattern::Default)
            }
            Some((Token::Identifier, _)) => {
                let name = self.parse_simple_identifier()?;
                Ok(Pattern::Identifier(name))
            }
            Some((Token::LParen, _)) => self.tuple_pattern(),
            Some((Token::Regex(_), _)) => {
                if let Literal::Regex(regex_token) = self.regex_literal()? {
                    Ok(Pattern::Regex(regex_token))
                } else {
                    unreachable!()
                }
            }
            _ if self.lookahead_is_literal() => {
                let literal = self.literal()?;
                Ok(Pattern::Literal(literal))
            }
            _ => Err(self
                .error_unexpected_lookahead_token("a pattern (literal, identifier, or default)")),
        }
    }

    /*
        TuplePattern
            : '(' (Pattern (',' Pattern)* ','?)? ')'
            ;
    */
    fn tuple_pattern(&mut self) -> Result<Pattern, SyntaxError> {
        self.eat_token(&Token::LParen)?;
        let mut patterns = Vec::new();
        if !self.lookahead_is_rparen() {
            patterns.push(self.pattern()?);
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.lookahead_is_rparen() {
                    break;
                } // Allow trailing comma
                patterns.push(self.pattern()?);
            }
        }
        self.eat_token(&Token::RParen)?;
        Ok(Pattern::Tuple(patterns))
    }

    /*
        ListLiteralExpression
            : '[' (Expression (',' Expression)* ','? )? ']'
            ;
    */
    fn list_literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBracket)?;

        let mut elements = vec![];
        while self.match_lookahead_type(|t| t != &Token::RBracket) {
            elements.push(self.expression()?);
            if !self.lookahead_is_comma() {
                break;
            }
            self.eat_token(&Token::Comma)?;
        }

        self.eat_token(&Token::RBracket)?;
        Ok(ast::list(elements))
    }

    /*
        BraceExpression
            : MapLiteralExpression
            | SetLiteralExpression
            ;
    */
    fn brace_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LBrace)?;

        // If the next token is a closing brace, it's an empty map.
        if self.match_lookahead_type(|t| t == &Token::RBrace) {
            self.eat_token(&Token::RBrace)?;
            return Ok(ast::map(vec![]));
        }

        // Parse the first expression.
        let first_expr = self.expression()?;

        // Look ahead for a colon to distinguish between a map and a set.
        if self.lookahead_is_colon() {
            // It's a map.
            self.eat_token(&Token::Colon)?;
            let first_value = self.expression()?;
            let mut pairs = vec![(first_expr, first_value)];

            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.match_lookahead_type(|t| t == &Token::RBrace) {
                    break;
                } // Trailing comma
                let key = self.expression()?;
                self.eat_token(&Token::Colon)?;
                let value = self.expression()?;
                pairs.push((key, value));
            }
            self.eat_token(&Token::RBrace)?;
            Ok(ast::map(pairs))
        } else {
            // It's a set.
            let mut elements = vec![first_expr];
            while self.lookahead_is_comma() {
                self.eat_token(&Token::Comma)?;
                if self.match_lookahead_type(|t| t == &Token::RBrace) {
                    break;
                } // Trailing comma
                elements.push(self.expression()?);
            }
            self.eat_token(&Token::RBrace)?;
            Ok(ast::set(elements))
        }
    }

    /*
        LambdaExpression
            : 'async'? 'gpu'? 'fn' [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] EXPRESSION_END BlockStatement
            | 'async'? 'gpu'? 'fn' [GenericTypesDeclaration] '(' ParameterList ')' [ReturnType] ':' ExpressionStatement EXPRESSION_END
            ;
    */
    fn lambda_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut properties = FunctionProperties {
            is_async: false,
            is_gpu: false,
            visibility: MemberVisibility::Public,
        };

        while self.lookahead_is_function_modifier() {
            match &self._lookahead {
                Some((Token::Async, _)) => {
                    self.eat_token(&Token::Async)?;
                    properties.is_async = true;
                }
                Some((Token::Gpu, _)) => {
                    self.eat_token(&Token::Gpu)?;
                    properties.is_gpu = true;
                }
                _ => break,
            }
        }

        self.eat_token(&Token::Fn)?;

        let generic_types = self.generic_types_expression()?;
        let parameters = self.function_params_expression()?;
        let return_type = self.return_type_expression()?;

        let body_parsing_error = self.error_unexpected_lookahead_token(
            "a colon for an inline body or an indented block for a block body",
        );
        let body = match &self._lookahead {
            Some((Token::Colon, _)) => {
                self.eat_token(&Token::Colon)?;
                let expr = self.expression()?;
                ast::expression_statement(expr)
            }
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    self.block_statement()?
                } else if self.lookahead_is_dedent() || self._lookahead.is_none() {
                    Statement::Empty // No body, just an expression end
                } else {
                    return Err(body_parsing_error);
                }
            }
            _ => return Err(body_parsing_error),
        };

        Ok(ast::lambda_expression(
            generic_types,
            parameters,
            return_type,
            body,
            properties,
        ))
    }

    /*
        ParenthesizedExpression
            : '(' Expression ')'
            ;
        TupleLiteralExpression
            : '(' (Expression (',' Expression)* ','? )? ')'
            ;
    */
    fn parenthesized_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.eat_token(&Token::LParen)?;

        // Handle the empty tuple `()` case.
        if self.match_lookahead_type(|t| t == &Token::RParen) {
            self.eat_token(&Token::RParen)?;
            return Ok(ast::tuple(vec![]));
        }

        let first_expr = self.expression()?;

        // The presence of a comma is what distinguishes a tuple from a grouping parenthesis.
        if !self.lookahead_is_comma() {
            // No comma, so this is a grouping parenthesized expression.
            self.eat_token(&Token::RParen)?;
            return Ok(first_expr);
        }

        // It's a tuple. Start with the first expression we already parsed.
        let mut elements = vec![first_expr];

        // Loop through the rest of the comma-separated expressions.
        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            // Handle optional trailing comma before the closing parenthesis.
            if self.match_lookahead_type(|t| t == &Token::RParen) {
                break;
            }
            elements.push(self.expression()?);
        }

        self.eat_token(&Token::RParen)?;
        Ok(ast::tuple(elements))
    }

    /*
        LiteralExpression
            : Literal
            ;
    */
    fn literal_expression(&mut self) -> Result<Expression, SyntaxError> {
        let span = if let Some((_, span)) = &self._lookahead {
            span.clone()
        } else {
            return Err(self.error_eof());
        };
        let literal = self.literal()?;
        Ok(ast::literal_with_span(literal, span))
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
            Some((Token::None, _)) => {
                self.eat_token(&Token::None)?;
                Ok(Literal::None)
            }
            Some((Token::String, _)) => self.string_literal(),
            Some((Token::Symbol, _)) => self.symbol_literal(),
            Some((Token::Regex(_), _)) => self.regex_literal(),
            Some((Token::FormattedStringStart(_), _))
            | Some((Token::FormattedStringMiddle(_), _))
            | Some((Token::FormattedStringEnd(_), _)) => {
                // These are handled by formatted_string_expression, not here.
                Err(self.error_unexpected_lookahead_token("a literal"))
            }
            Some((token, span)) => {
                let token_text = &self.source[span.start..span.end];
                Err(self.error_unexpected_token_with_span(
                    "a valid literal",
                    &format!("{:?} with value '{}'", token, token_text),
                    span.clone(),
                ))
            }
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
                            token.1.start..token.1.end,
                        )
                    })?,
                    Token::BinaryNumber => {
                        // Strip "0b" prefix and parse as base 2
                        i128::from_str_radix(&str_value[2..], 2).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidBinaryLiteral,
                                token.1.start..token.1.end,
                            )
                        })?
                    }
                    Token::HexNumber => {
                        // Strip "0x" prefix and parse as base 16
                        i128::from_str_radix(&str_value[2..], 16).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidHexLiteral,
                                token.1.start..token.1.end,
                            )
                        })?
                    }
                    Token::OctalNumber => {
                        // Strip "0o" prefix and parse as base 8
                        i128::from_str_radix(&str_value[2..], 8).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidOctalLiteral,
                                token.1.start..token.1.end,
                            )
                        })?
                    }
                    _ => {
                        return Err(SyntaxError::new(
                            SyntaxErrorKind::UnexpectedToken {
                                expected: "integer literal".to_string(),
                                found: format!("{:?}", token_type),
                            },
                            token.1.start..token.1.end,
                        ))
                    }
                };

                Ok(ast::int_literal(value))
            }
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
                let err = SyntaxError::new(
                    SyntaxErrorKind::InvalidFloatLiteral,
                    token.1.start..token.1.end,
                );
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                let f32_value = str_value.parse::<f32>().map_err(|_| err.clone())?;
                let uses_exponent = str_value.contains('e') || str_value.contains('E');
                let f32_str = if uses_exponent {
                    // Count digits after the decimal in the significand (before 'e')
                    let significand = str_value.split(['e', 'E']).next().unwrap_or("");
                    let decimal_digits = significand.split('.').nth(1).unwrap_or("").len();
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
                    Ok(ast::float32_literal(f32_value))
                } else {
                    // Otherwise, parse as f64
                    let f64_value = str_value.parse::<f64>().map_err(|_| err.clone())?;
                    if f64_value.is_finite() {
                        Ok(ast::float64_literal(f64_value))
                    } else {
                        Err(err)
                    }
                }
            }
            Err(e) => Err(e),
        }
    }

    /*
        StringLiteral
            : DoubleQuotedString
            : SingleQuotedString
            ;
    */
    fn string_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::String) {
            Ok(token) => {
                let mut str_value = &self.source[token.1.start..token.1.end];

                // Strings that come from f-string expressions will have escaped quotes.
                if str_value.starts_with('\\') {
                    str_value = &str_value[2..str_value.len() - 1];
                } else {
                    str_value = &str_value[1..str_value.len() - 1];
                }

                let literal = ast::string_literal(str_value);
                Ok(literal)
            }
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
                    "true" => ast::boolean(true),
                    "false" => ast::boolean(false),
                    _ => {
                        return Err(SyntaxError::new(
                            SyntaxErrorKind::InvalidBooleanLiteral,
                            token.1.start..token.1.end,
                        ))
                    }
                };
                Ok(literal)
            }
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
                let literal = ast::symbol(str_value);
                Ok(literal)
            }
            Err(e) => Err(e),
        }
    }

    /*
        RegexLiteral
            : REGEX
            ;
    */
    fn regex_literal(&mut self) -> Result<Literal, SyntaxError> {
        let token_span = self.eat(|t| matches!(t, Token::Regex(_)), "regex literal")?;
        if let (Token::Regex(regex_data), _) = token_span {
            Ok(ast::regex_literal_from_token(regex_data))
        } else {
            // This branch should be unreachable if the predicate in `eat` is correct.
            unreachable!();
        }
    }

    fn eat(
        &mut self,
        expected: impl Fn(&Token) -> bool,
        expected_str: &str,
    ) -> Result<TokenSpan, SyntaxError> {
        let token = &self._lookahead;

        match token {
            Some((ref t, ref span)) if expected(t) => {
                let result = (t.clone(), span.clone());
                self._lookahead = self.lexer.next().transpose()?;
                Ok(result)
            }
            Some((found, _)) => Err(SyntaxError::new(
                SyntaxErrorKind::UnexpectedToken {
                    expected: expected_str.to_string(),
                    found: token_to_string(found),
                },
                self.source.len()..self.source.len(),
            )),
            None => {
                if expected(&Token::ExpressionStatementEnd) {
                    // Special case for end of expression
                    self._lookahead = None;
                    return Ok((Token::ExpressionStatementEnd, 0..0));
                }

                Err(self.error_eof())
            }
        }
    }

    fn eat_token(&mut self, expected: &Token) -> Result<TokenSpan, SyntaxError> {
        self.eat(|t| t == expected, &token_to_string(expected))
    }

    fn eat_binary_op(&mut self, match_token: fn(&Token) -> bool) -> Result<TokenSpan, SyntaxError> {
        self.eat(match_token, "binary operator")
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
        self._lookahead
            .as_ref()
            .map_or("end of file".to_string(), |(t, _)| token_to_string(t))
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
                _ => return Err(Err(self.error_unexpected_operator(token, "+, -, |, &, ^"))),
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
                _ => return Err(Err(self.error_unexpected_operator(token, "<, <=, >, >="))),
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
                _ => return Err(Err(self.error_unexpected_operator(token, "=, !="))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_and_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_and_op) {
            Ok(token) => match token.0 {
                Token::And => BinaryOp::And,
                _ => return Err(Err(self.error_unexpected_operator(token, "and"))),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_or_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_or_op) {
            Ok(token) => match token.0 {
                Token::Or => BinaryOp::Or,
                _ => return Err(Err(self.error_unexpected_operator(token, "or"))),
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
                _ => return Err(Err(self.error_unexpected_operator(token, "*, /, %"))),
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

    fn eat_statement_end(&mut self) -> Result<(), SyntaxError> {
        // A statement must be followed by a token that can validly end it.
        // This includes a newline, the end of the file, the end of a block (Dedent),
        // or a keyword that starts a new clause (like `else`).
        match &self._lookahead {
            // Valid terminators that we consume.
            Some((Token::ExpressionStatementEnd, _)) => {
                self.eat_expression_end()?;
            }
            // Valid terminators that we DON'T consume, as they belong to other parsers.
            None
            | Some((Token::Dedent, _))
            | Some((Token::Else, _))
            | Some((Token::While, _))
            | Some((Token::Until, _)) => {
                // Do nothing, the token is a valid boundary.
            }
            // Anything else is an error.
            _ => {
                return Err(self.error_unexpected_lookahead_token("an end of statement"));
            }
        }
        Ok(())
    }

    fn error_unexpected_operator(&self, token: TokenSpan, expected: &str) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: self.lookahead_as_string(),
            },
            token.1.start..token.1.end,
        )
    }

    fn error_unexpected_token(&self, expected: &str, found: &str) -> SyntaxError {
        self.error_unexpected_token_with_span(expected, found, self.source.len()..self.source.len())
    }

    fn error_invalid_inheritance_identifier(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidInheritanceIdentifier,
            self.source.len()..self.source.len(),
        )
    }

    fn error_unexpected_token_with_span(
        &self,
        expected: &str,
        found: &str,
        span: Span,
    ) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: found.to_string(),
            },
            span,
        )
    }

    fn error_unexpected_lookahead_token(&self, expected: &str) -> SyntaxError {
        self.error_unexpected_token(expected, &self.lookahead_as_string())
    }

    fn error_missing_match_branches(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingMatchBranches,
            self.source.len()..self.source.len(),
        )
    }

    fn error_duplicate_match_pattern(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::DuplicateMatchPattern,
            self.source.len()..self.source.len(),
        )
    }

    fn error_eof(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedEOF,
            self.source.len()..self.source.len(),
        )
    }

    fn error_invalid_left_hand_side_expression(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidLeftHandSideExpression,
            self.source.len()..self.source.len(),
        )
    }

    fn error_invalid_type_declaration(&self, expected: &str) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidTypeDeclaration {
                expected: expected.to_string(),
            },
            self.source.len()..self.source.len(),
        )
    }

    fn error_missing_struct_member_type(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingStructMemberType,
            self.source.len()..self.source.len(),
        )
    }

    fn error_missing_members(&self, kind: SyntaxErrorKind) -> SyntaxError {
        SyntaxError::new(kind, self.source.len()..self.source.len())
    }

    fn error_missing_type_expression(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::MissingTypeExpression,
            self.source.len()..self.source.len(),
        )
    }
}

fn is_additive_op(token: &Token) -> bool {
    matches!(
        token,
        Token::Plus | Token::Minus | Token::Pipe | Token::Ampersand | Token::Caret
    )
}

fn is_relational_op(token: &Token) -> bool {
    matches!(
        token,
        Token::LessThan | Token::LessThanEqual | Token::GreaterThanEqual | Token::GreaterThan
    )
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
    matches!(
        token,
        Token::Assign
            | Token::AssignAdd
            | Token::AssignSub
            | Token::AssignMul
            | Token::AssignDiv
            | Token::AssignMod
    )
}

fn is_literal(token: &Token) -> bool {
    matches!(
        token,
        Token::Int
            | Token::BinaryNumber
            | Token::HexNumber
            | Token::OctalNumber
            | Token::Float
            | Token::True
            | Token::False
            | Token::String
            | Token::Symbol
            | Token::Regex(_)
            | Token::None
    )
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
    matches!(
        token,
        Token::GreaterThan
            | Token::GreaterThanEqual
            | Token::LessThan
            | Token::LessThanEqual
            | Token::In
            | Token::Not
            | Token::NotEqual
    )
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
